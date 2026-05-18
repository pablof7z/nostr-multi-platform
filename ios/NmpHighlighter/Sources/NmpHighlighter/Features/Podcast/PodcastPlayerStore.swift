import AVFoundation
import Foundation
import MediaPlayer
import Observation
import os
import UIKit

private struct PositionRecord: Codable {
    var guid: String
    var position: Double
    var lastPlayedAt: Date
    /// Minimal snapshot for cold-launch rehydration so the MiniPlayer can show
    /// the last episode (paused) without waiting on relay sync. Once the user
    /// taps play, we still go through `load(artifact:)` to wire AVPlayer.
    var snapshot: ArtifactSnapshot?
}

private struct ChapterSnapshot: Codable {
    var startSeconds: Double
    var title: String
}

private struct ArtifactSnapshot: Codable {
    var title: String
    var image: String
    var podcastShowTitle: String
    var podcastItemGuid: String
    var podcastGuid: String
    var audioUrl: String
    var audioPreviewUrl: String
    var transcriptUrl: String
    var durationSeconds: Int64?
    var groupId: String
    var shareEventId: String
    var pubkey: String
    var createdAt: UInt64?
    var note: String
    var chapters: [ChapterSnapshot]

    init(from record: ArtifactRecord) {
        self.title = record.preview.title
        self.image = record.preview.image
        self.podcastShowTitle = record.preview.podcastShowTitle
        self.podcastItemGuid = record.preview.podcastItemGuid
        self.podcastGuid = record.preview.podcastGuid
        self.audioUrl = record.preview.audioUrl
        self.audioPreviewUrl = record.preview.audioPreviewUrl
        self.transcriptUrl = record.preview.transcriptUrl
        self.durationSeconds = record.preview.durationSeconds
        self.groupId = record.groupId
        self.shareEventId = record.shareEventId
        self.pubkey = record.pubkey
        self.createdAt = record.createdAt
        self.note = record.note
        self.chapters = record.preview.chapters.map {
            ChapterSnapshot(startSeconds: $0.startSeconds, title: $0.title)
        }
    }

    func materialize() -> ArtifactRecord {
        let preview = ArtifactPreview(
            id: shareEventId,
            url: "",
            title: title,
            author: "",
            image: image,
            description: "",
            source: "podcast",
            domain: "",
            catalogId: podcastItemGuid.isEmpty ? podcastGuid : podcastItemGuid,
            catalogKind: podcastItemGuid.isEmpty
                ? (podcastGuid.isEmpty ? "" : "podcast:guid")
                : "podcast:item:guid",
            podcastGuid: podcastGuid,
            podcastItemGuid: podcastItemGuid,
            podcastShowTitle: podcastShowTitle,
            audioUrl: audioUrl,
            audioPreviewUrl: audioPreviewUrl,
            transcriptUrl: transcriptUrl,
            feedUrl: "",
            publishedAt: "",
            durationSeconds: durationSeconds,
            referenceTagName: "i",
            referenceTagValue: podcastItemGuid.isEmpty
                ? (podcastGuid.isEmpty ? "" : "podcast:guid:\(podcastGuid)")
                : "podcast:item:guid:\(podcastItemGuid)",
            referenceKind: podcastItemGuid.isEmpty
                ? (podcastGuid.isEmpty ? "" : "podcast:guid")
                : "podcast:item:guid",
            highlightTagName: "",
            highlightTagValue: "",
            highlightReferenceKey: "",
            chapters: chapters.map { Chapter(startSeconds: $0.startSeconds, title: $0.title) }
        )
        return ArtifactRecord(
            preview: preview,
            groupId: groupId,
            shareEventId: shareEventId,
            pubkey: pubkey,
            createdAt: createdAt,
            note: note
        )
    }
}

@MainActor
@Observable
final class PodcastPlayerStore {
    // MARK: - Observable state

    private(set) var currentArtifact: ArtifactRecord?
    private(set) var audioUrl: URL?
    private(set) var currentTime: TimeInterval = 0
    private(set) var duration: TimeInterval = 0
    private(set) var isPlaying: Bool = false
    private(set) var isBuffering: Bool = false
    private(set) var loadedTimeRanges: [ClosedRange<TimeInterval>] = []
    private(set) var lastError: String?
    private(set) var clipStart: TimeInterval?
    private(set) var clipEnd: TimeInterval?
    var speaker: String = ""
    private(set) var selectedSegmentIds: Set<String> = []
    private(set) var isPublishing: Bool = false
    private(set) var publishError: String?

    // Global transcript state
    private(set) var transcriptSegments: [TranscriptSegment] = []
    private(set) var transcriptAvailability: TranscriptAvailability = .unavailable

    // Clip comment cache keyed by clip event id
    var comments: [String: [CommentRecord]] = [:]

    // Apple Music–style: only one clip expanded at a time
    var expandedClipId: String? = nil

    /// One-peak-per-second amplitude envelope (0...1) for the loaded episode.
    /// Empty until extraction completes; nil after extraction was attempted
    /// but skipped (cellular, format unsupported, etc.). Used by the
    /// listening view's tick rows to show a real waveform instead of a
    /// placeholder.
    private(set) var waveformPeaks: [Float] = []

    // MARK: - Private plumbing

    @ObservationIgnored private var player: AVPlayer?
    @ObservationIgnored private let logger = Logger(subsystem: "com.highlighter.app", category: "PodcastPlayer")
    @ObservationIgnored private nonisolated(unsafe) var timeObserver: Any?
    @ObservationIgnored private nonisolated(unsafe) var statusObserver: NSKeyValueObservation?
    @ObservationIgnored private nonisolated(unsafe) var bufferingObserver: NSKeyValueObservation?
    @ObservationIgnored private nonisolated(unsafe) var rangesObserver: NSKeyValueObservation?
    @ObservationIgnored private nonisolated(unsafe) var errorObserver: NSKeyValueObservation?
    @ObservationIgnored private nonisolated(unsafe) var playbackEndObserver: NSObjectProtocol?
    @ObservationIgnored private var positionPersistenceTask: Task<Void, Never>?
    @ObservationIgnored private var transcriptTask: Task<Void, Never>?
    @ObservationIgnored private var waveformTask: Task<Void, Never>?

    private static let positionDefaultsKey = "highlighter.podcast.lastPosition"

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

        startPositionPersistence()

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

    // MARK: - Position persistence

    private func startPositionPersistence() {
        positionPersistenceTask?.cancel()
        positionPersistenceTask = Task {
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 5 * 1_000_000_000)
                guard !Task.isCancelled else { break }
                persistPosition()
            }
        }
    }

    private func persistPosition() {
        guard let artifact = currentArtifact, isPlaying else { return }
        let guid = artifact.preview.podcastItemGuid
        guard !guid.isEmpty else { return }
        let record = PositionRecord(
            guid: guid,
            position: currentTime,
            lastPlayedAt: Date(),
            snapshot: ArtifactSnapshot(from: artifact)
        )
        if let data = try? JSONEncoder().encode(record) {
            UserDefaults.standard.set(data, forKey: Self.positionDefaultsKey)
        }
    }

    private func loadPositionRecord() -> PositionRecord? {
        guard let data = UserDefaults.standard.data(forKey: Self.positionDefaultsKey) else { return nil }
        return try? JSONDecoder().decode(PositionRecord.self, from: data)
    }

    /// Cold-launch rehydration. Surfaces the MiniPlayer in a paused state with
    /// the last episode the user listened to (within the last 7 days). The
    /// AVPlayer is NOT created — that happens when the user taps play and we
    /// route through `load(artifact:)` which seeks to the saved position.
    func rehydrateFromSavedRecord() {
        guard currentArtifact == nil else { return }
        guard let record = loadPositionRecord(), let snapshot = record.snapshot else { return }
        let age = Date().timeIntervalSince(record.lastPlayedAt)
        guard age < 7 * 24 * 3600 else { return }
        currentArtifact = snapshot.materialize()
        currentTime = record.position
        if let dur = snapshot.durationSeconds, dur > 0 {
            duration = TimeInterval(dur)
        }
        isPlaying = false
    }

    // MARK: - Player setup helpers

    private func installTimeObserver(on player: AVPlayer) {
        let interval = CMTime(seconds: 0.25, preferredTimescale: 600)
        timeObserver = player.addPeriodicTimeObserver(forInterval: interval, queue: .main) { [weak self] time in
            MainActor.assumeIsolated {
                guard let self else { return }
                let seconds = time.seconds.isFinite ? time.seconds : 0
                let previousWhole = Int(self.currentTime)
                self.currentTime = seconds
                // Update Now Playing elapsed time once per second to keep the
                // lock screen scrubber accurate without excessive churn.
                if Int(seconds) != previousWhole {
                    self.updateNowPlayingInfo()
                }
            }
        }
    }

    private func observeItem(_ item: AVPlayerItem) {
        statusObserver = item.observe(\.status, options: [.initial, .new]) { [weak self, weak item] _, _ in
            guard let self, let item else { return }
            Task { @MainActor in
                let status = item.status
                self.logger.info("item status=\(status.rawValue)")
                guard status == .readyToPlay else { return }
                do {
                    let loaded = try await item.asset.load(.duration)
                    let seconds = loaded.seconds
                    if seconds.isFinite, seconds > 0 {
                        self.duration = seconds
                        self.logger.info("duration=\(seconds, format: .fixed(precision: 1))s")
                        self.updateNowPlayingInfo()
                    }
                } catch {
                    self.logger.error("duration load failed: \(error.localizedDescription, privacy: .public)")
                }
            }
        }
    }

    private func observeBuffering(_ item: AVPlayerItem) {
        bufferingObserver = item.observe(
            \.isPlaybackLikelyToKeepUp,
            options: [.initial, .new]
        ) { [weak self, weak item] _, _ in
            guard let self, let item else { return }
            Task { @MainActor in
                let likelyToKeepUp = item.isPlaybackLikelyToKeepUp
                let bufferEmpty = item.isPlaybackBufferEmpty
                let newBuffering = !likelyToKeepUp && !bufferEmpty
                if self.isBuffering != newBuffering {
                    self.logger.info("buffering=\(newBuffering) likelyToKeepUp=\(likelyToKeepUp) bufferEmpty=\(bufferEmpty)")
                    self.isBuffering = newBuffering
                }
            }
        }
    }

    private func observeLoadedRanges(_ item: AVPlayerItem) {
        rangesObserver = item.observe(
            \.loadedTimeRanges,
            options: [.initial, .new]
        ) { [weak self, weak item] _, _ in
            guard let self, let item else { return }
            let ranges = item.loadedTimeRanges.compactMap { value -> ClosedRange<TimeInterval>? in
                let range = value.timeRangeValue
                let start = range.start.seconds
                let end = CMTimeRangeGetEnd(range).seconds
                guard start.isFinite, end.isFinite, end > start else { return nil }
                return start...end
            }
            Task { @MainActor in
                self.loadedTimeRanges = ranges
            }
        }
    }

    private func observeError(_ item: AVPlayerItem) {
        errorObserver = item.observe(\.error, options: [.new]) { [weak self, weak item] _, _ in
            guard let self, let item else { return }
            Task { @MainActor in
                if let error = item.error {
                    let msg = error.localizedDescription
                    self.logger.error("playback error: \(msg, privacy: .public)")
                    self.lastError = msg
                    self.isPlaying = false
                }
            }
        }
    }

    private func observePlaybackEnd(item: AVPlayerItem) {
        playbackEndObserver = NotificationCenter.default.addObserver(
            forName: .AVPlayerItemDidPlayToEndTime,
            object: item,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.isPlaying = false
            }
        }
    }

    // MARK: - Remote Command Center

    /// Call once per loaded episode. Registers play/pause/skip/seek handlers
    /// on MPRemoteCommandCenter so the lock screen and Control Center controls
    /// actually work.
    private func configureRemoteCommandCenter() {
        let center = MPRemoteCommandCenter.shared()

        center.playCommand.isEnabled = true
        center.playCommand.addTarget { [weak self] _ in
            self?.play()
            return .success
        }

        center.pauseCommand.isEnabled = true
        center.pauseCommand.addTarget { [weak self] _ in
            self?.pause()
            return .success
        }

        center.togglePlayPauseCommand.isEnabled = true
        center.togglePlayPauseCommand.addTarget { [weak self] _ in
            self?.toggle()
            return .success
        }

        center.skipForwardCommand.isEnabled = true
        center.skipForwardCommand.preferredIntervals = [30]
        center.skipForwardCommand.addTarget { [weak self] event in
            guard let self, let e = event as? MPSkipIntervalCommandEvent else { return .commandFailed }
            skip(by: e.interval)
            return .success
        }

        center.skipBackwardCommand.isEnabled = true
        center.skipBackwardCommand.preferredIntervals = [15]
        center.skipBackwardCommand.addTarget { [weak self] event in
            guard let self, let e = event as? MPSkipIntervalCommandEvent else { return .commandFailed }
            skip(by: -e.interval)
            return .success
        }

        center.changePlaybackPositionCommand.isEnabled = true
        center.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let self, let e = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            seek(to: e.positionTime)
            return .success
        }

        // Lock Screen custom actions note:
        // iOS does not expose a public API for adding arbitrary buttons (e.g.
        // "Clip") to the Now Playing lock-screen widget or Control Center.
        // MPRemoteCommandCenter only exposes a fixed set of well-known
        // commands. Lock Screen Widgets (WidgetKit) cannot interact with an
        // in-process media player. A Now Playing ActivityExtension / Live
        // Activity could show metadata but still cannot inject custom
        // commands. Therefore a "Clip" lock screen button is not viable with
        // current public APIs.
    }

    private func tearDownRemoteCommandCenter() {
        let center = MPRemoteCommandCenter.shared()
        center.playCommand.removeTarget(nil)
        center.pauseCommand.removeTarget(nil)
        center.togglePlayPauseCommand.removeTarget(nil)
        center.skipForwardCommand.removeTarget(nil)
        center.skipBackwardCommand.removeTarget(nil)
        center.changePlaybackPositionCommand.removeTarget(nil)
    }

    // MARK: - Now Playing Info Center

    /// Pushes current episode metadata + playback state to the system's
    /// Now Playing Info Center. Call whenever playback state or position
    /// changes. This drives the lock screen and Control Center artwork, title,
    /// progress bar, and elapsed/remaining counters.
    private func updateNowPlayingInfo(artwork: MPMediaItemArtwork? = nil) {
        guard let artifact = currentArtifact else {
            MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
            return
        }

        var info: [String: Any] = [:]
        info[MPMediaItemPropertyTitle] = artifact.preview.title.isEmpty ? "Untitled episode" : artifact.preview.title
        info[MPMediaItemPropertyArtist] = artifact.preview.podcastShowTitle.isEmpty
            ? artifact.preview.author
            : artifact.preview.podcastShowTitle
        info[MPMediaItemPropertyMediaType] = MPMediaType.podcast.rawValue

        if duration > 0 {
            info[MPMediaItemPropertyPlaybackDuration] = duration
        }
        info[MPNowPlayingInfoPropertyElapsedPlaybackTime] = currentTime
        info[MPNowPlayingInfoPropertyPlaybackRate] = isPlaying ? 1.0 : 0.0
        info[MPNowPlayingInfoPropertyDefaultPlaybackRate] = 1.0

        if let artwork {
            info[MPMediaItemPropertyArtwork] = artwork
        } else if let existing = MPNowPlayingInfoCenter.default().nowPlayingInfo?[MPMediaItemPropertyArtwork] {
            // Preserve previously loaded artwork while async fetch runs.
            info[MPMediaItemPropertyArtwork] = existing
        }

        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    }

    /// Fetches episode artwork from the network and updates Now Playing Info.
    /// Runs entirely off the main thread; hops back to update state.
    private func fetchAndApplyArtwork(from urlString: String) {
        guard !urlString.isEmpty, let url = URL(string: urlString) else { return }
        Task(priority: .userInitiated) { [weak self] in
            guard let data = try? Data(contentsOf: url),
                  let uiImage = UIImage(data: data) else { return }
            let artwork = MPMediaItemArtwork(boundsSize: uiImage.size) { _ in uiImage }
            await MainActor.run { [weak self] in
                self?.updateNowPlayingInfo(artwork: artwork)
            }
        }
    }

    private func tearDownPlayer() {
        positionPersistenceTask?.cancel()
        positionPersistenceTask = nil
        transcriptTask?.cancel()
        transcriptTask = nil
        waveformTask?.cancel()
        waveformTask = nil

        if let player, let timeObserver {
            player.removeTimeObserver(timeObserver)
        }
        timeObserver = nil
        statusObserver?.invalidate()
        statusObserver = nil
        bufferingObserver?.invalidate()
        bufferingObserver = nil
        rangesObserver?.invalidate()
        rangesObserver = nil
        errorObserver?.invalidate()
        errorObserver = nil
        if let playbackEndObserver {
            NotificationCenter.default.removeObserver(playbackEndObserver)
        }
        playbackEndObserver = nil
        player?.pause()
        player = nil

        tearDownRemoteCommandCenter()
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
    }
}

enum TranscriptAvailability {
    case loading, available, unavailable
}

enum PodcastPlayerError: Error, LocalizedError {
    case emptyResult

    var errorDescription: String? {
        switch self {
        case .emptyResult: return "No highlight returned from publish."
        }
    }
}
