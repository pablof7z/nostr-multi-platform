import AVFoundation
import Foundation
import MediaPlayer
import Observation
import os
import UIKit
@Observable
final class PodcastPlayerStore {
    // MARK: - Observable state

    var currentArtifact: ArtifactRecord?
    var audioUrl: URL?
    var currentTime: TimeInterval = 0
    var duration: TimeInterval = 0
    var isPlaying: Bool = false
    var isBuffering: Bool = false
    var loadedTimeRanges: [ClosedRange<TimeInterval>] = []
    var lastError: String?
    var clipStart: TimeInterval?
    var clipEnd: TimeInterval?
    var speaker: String = ""
    var selectedSegmentIds: Set<String> = []
    var isPublishing: Bool = false
    var publishError: String?

    // Global transcript state
    var transcriptSegments: [TranscriptSegment] = []
    var transcriptAvailability: TranscriptAvailability = .unavailable

    // Clip comment cache keyed by clip event id
    var comments: [String: [CommentRecord]] = [:]

    // Apple Music–style: only one clip expanded at a time
    var expandedClipId: String? = nil

    /// One-peak-per-second amplitude envelope (0...1) for the loaded episode.
    /// Empty until extraction completes; nil after extraction was attempted
    /// but skipped (cellular, format unsupported, etc.). Used by the
    /// listening view's tick rows to show a real waveform instead of a
    /// placeholder.
    var waveformPeaks: [Float] = []

    // MARK: - Private plumbing

    @ObservationIgnored var player: AVPlayer?
    @ObservationIgnored let logger = Logger(subsystem: "com.highlighter.app", category: "PodcastPlayer")
    @ObservationIgnored nonisolated(unsafe) var timeObserver: Any?
    @ObservationIgnored nonisolated(unsafe) var statusObserver: NSKeyValueObservation?
    @ObservationIgnored nonisolated(unsafe) var bufferingObserver: NSKeyValueObservation?
    @ObservationIgnored nonisolated(unsafe) var rangesObserver: NSKeyValueObservation?
    @ObservationIgnored nonisolated(unsafe) var errorObserver: NSKeyValueObservation?
    @ObservationIgnored nonisolated(unsafe) var playbackEndObserver: NSObjectProtocol?
    @ObservationIgnored var transcriptTask: Task<Void, Never>?
    @ObservationIgnored var waveformTask: Task<Void, Never>?

    static let positionDefaultsKey = "highlighter.podcast.lastPosition"

    // MARK: - Lifecycle

    deinit {
        // Access only nonisolated(unsafe) properties here — no MainActor hop in deinit.
        if let player, let timeObserver {
            player.removeTimeObserver(timeObserver)
        }
        statusObserver?.invalidate()
        bufferingObserver?.invalidate()
        rangesObserver?.invalidate()
        errorObserver?.invalidate()
        if let playbackEndObserver {
            NotificationCenter.default.removeObserver(playbackEndObserver)
        }
        player?.pause()
    }

    // MARK: - Global load / clear

    func load(artifact: ArtifactRecord) {
        let audioCandidate = artifact.preview.audioUrl.isEmpty
            ? artifact.preview.audioPreviewUrl
            : artifact.preview.audioUrl
        guard !audioCandidate.isEmpty, let url = URL(string: audioCandidate) else {
            logger.warning("load: no usable audio URL for artifact \(artifact.shareEventId, privacy: .public)")
            return
        }

        // If same episode is already loaded, just play.
        if currentArtifact?.shareEventId == artifact.shareEventId {
            play()
            return
        }

        tearDownPlayer()

        currentArtifact = artifact
        self.audioUrl = url
        lastError = nil
        isBuffering = false
        loadedTimeRanges = []
        transcriptSegments = []
        transcriptAvailability = .unavailable
        clipStart = nil
        clipEnd = nil
        selectedSegmentIds.removeAll()
        speaker = ""
        publishError = nil
        currentTime = 0
        duration = 0

        logger.info("load artifact=\(artifact.shareEventId, privacy: .public) url=\(url.absoluteString, privacy: .public)")

        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .spokenAudio)
        try? AVAudioSession.sharedInstance().setActive(true)

        let item = AVPlayerItem(url: url)
        item.preferredForwardBufferDuration = 10

        let newPlayer = AVPlayer(playerItem: item)
        newPlayer.automaticallyWaitsToMinimizeStalling = true
        self.player = newPlayer

        installTimeObserver(on: newPlayer)
        observeItem(item)
        observeBuffering(item)
        observeLoadedRanges(item)
        observeError(item)
        observePlaybackEnd(item: item)

        // Resume saved position if guid matches.
        let savedGuid = artifact.preview.podcastItemGuid
        if !savedGuid.isEmpty, let record = loadPositionRecord(), record.guid == savedGuid {
            let age = Date().timeIntervalSince(record.lastPlayedAt)
            if age < 7 * 24 * 3600 {
                let seekTime = CMTime(seconds: record.position, preferredTimescale: 600)
                newPlayer.seek(to: seekTime, toleranceBefore: .zero, toleranceAfter: .zero)
                currentTime = record.position
            }
        }

        newPlayer.play()
        isPlaying = true

        configureRemoteCommandCenter()
        updateNowPlayingInfo()
        fetchAndApplyArtwork(from: artifact.preview.image)

        let transcriptUrl = artifact.preview.transcriptUrl
        if !transcriptUrl.isEmpty, let tUrl = URL(string: transcriptUrl) {
            transcriptAvailability = .loading
            transcriptTask = Task { await loadTranscript(from: tUrl) }
        }

        // Background: extract or load-from-cache the audio waveform. The
        // listening view falls back to plain time pegs when peaks aren't
        // present, so playback isn't blocked by this work.
        waveformPeaks = []
        waveformTask?.cancel()
        let dur = artifact.preview.durationSeconds.map(TimeInterval.init) ?? 0
        waveformTask = Task(priority: .background) { [weak self, url] in
            let peaks = await WaveformExtractor.peaks(forAudioURL: url, durationSeconds: dur)
            guard let self, !Task.isCancelled, let peaks else { return }
            await MainActor.run { self.waveformPeaks = peaks }
        }
    }

    /// Returns the 0...1 amplitude peak nearest the given timestamp, or nil
    /// when no waveform is loaded.
    func waveformPeak(at seconds: Double) -> Float? {
        guard !waveformPeaks.isEmpty else { return nil }
        let idx = max(0, min(waveformPeaks.count - 1, Int(seconds.rounded())))
        return waveformPeaks[idx]
    }

    /// Returns the slice of peaks covering [start, end) seconds. Used by the
    /// 30-second tick rows to render a mini-histogram of the actual audio.
    func waveformPeaks(from start: Double, to end: Double) -> [Float] {
        guard !waveformPeaks.isEmpty, end > start else { return [] }
        let lo = max(0, Int(start.rounded()))
        let hi = min(waveformPeaks.count, Int(end.rounded()))
        guard lo < hi else { return [] }
        return Array(waveformPeaks[lo..<hi])
    }

    func clear() {
        tearDownPlayer()
        currentArtifact = nil
        audioUrl = nil
        currentTime = 0
        duration = 0
        isPlaying = false
        isBuffering = false
        loadedTimeRanges = []
        lastError = nil
        clipStart = nil
        clipEnd = nil
        selectedSegmentIds.removeAll()
        speaker = ""
        publishError = nil
        transcriptSegments = []
        transcriptAvailability = .unavailable
        waveformPeaks = []
    }

    // MARK: - Transport

    func play() {
        // Cold-launch case: MiniPlayer was rehydrated from disk but AVPlayer
        // hasn't been created yet. Route through `load` to wire it up; the
        // saved-position branch in `load` will seek us back to where we were.
        if player == nil, let artifact = currentArtifact {
            logger.info("play (cold-launch rehydrate)")
            load(artifact: artifact)
            return
        }
        logger.info("play")
        player?.play()
        isPlaying = true
        updateNowPlayingInfo()
    }

    func pause() {
        logger.info("pause")
        player?.pause()
        isPlaying = false
        updateNowPlayingInfo()
    }

    func toggle() {
        if isPlaying { pause() } else { play() }
    }

    func seek(to seconds: TimeInterval) {
        let clamped = max(0, duration > 0 ? min(seconds, duration) : seconds)
        let time = CMTime(seconds: clamped, preferredTimescale: 600)
        player?.seek(to: time, toleranceBefore: .zero, toleranceAfter: .zero)
        currentTime = clamped
    }

    func skip(by delta: TimeInterval) {
        seek(to: currentTime + delta)
    }

    // MARK: - Clip selection

    func markIn() {
        clipStart = currentTime
        if let end = clipEnd, end < currentTime { clipEnd = nil }
    }

    func markOut() {
        clipEnd = currentTime
        if let start = clipStart, start > currentTime { clipStart = nil }
    }

    func clearClip() {
        clipStart = nil
        clipEnd = nil
        selectedSegmentIds.removeAll()
        speaker = ""
    }

    func extendClipToSegment(_ segment: TranscriptSegment) {
        let start = clipStart.map { min($0, segment.start) } ?? segment.start
        let end = clipEnd.map { max($0, segment.end) } ?? segment.end
        clipStart = start
        clipEnd = end
        selectedSegmentIds.insert(segment.id)
        if speaker.isEmpty, !segment.speaker.isEmpty {
            speaker = segment.speaker
        }
    }

    func setClipStart(_ value: TimeInterval) {
        var next = max(0, value)
        if let end = clipEnd { next = min(next, max(0, end - 0.05)) }
        clipStart = next
    }

    func setClipEnd(_ value: TimeInterval) {
        var next = duration > 0 ? min(value, duration) : value
        if let start = clipStart { next = max(next, start + 0.05) }
        clipEnd = next
    }

    // MARK: - Publish

    func publish(
        artifact: ArtifactRecord,
        targetGroupId: String,
        note: String,
        segments: [TranscriptSegment],
        core: SafeHighlighterCore
    ) async throws -> HighlightRecord {
        isPublishing = true
        publishError = nil
        defer { isPublishing = false }

        let selected = segments
            .filter { selectedSegmentIds.contains($0.id) }
            .sorted { $0.start < $1.start }
        let quote = selected.map(\.text).joined(separator: " ")

        let draft = HighlightDraft(
            quote: quote,
            context: "",
            note: note,
            clipStartSeconds: clipStart,
            clipEndSeconds: clipEnd,
            clipSpeaker: speaker,
            clipTranscriptSegmentIds: Array(selectedSegmentIds),
            image: nil
        )

        do {
            let results = try await core.publishHighlightsAndShare(
                artifact: artifact,
                drafts: [draft],
                targetGroupId: targetGroupId
            )
            guard let first = results.first else {
                throw PodcastPlayerError.emptyResult
            }
            return first
        } catch {
            publishError = "\(error)"
            throw error
        }
    }

    // MARK: - Transcript

    func loadTranscript(from url: URL) async {
        transcriptAvailability = .loading
        do {
            let (data, response) = try await URLSession.shared.data(from: url)
            let contentType = (response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Type")
            let ext = url.pathExtension.isEmpty ? nil : url.pathExtension
            let parsed = TranscriptParser.parse(
                data: data,
                contentType: contentType,
                fileExtension: ext
            )
            transcriptSegments = parsed
            transcriptAvailability = parsed.isEmpty ? .unavailable : .available
        } catch {
            logger.error("transcript load failed: \(error.localizedDescription, privacy: .public)")
            transcriptAvailability = .unavailable
        }
    }

}
