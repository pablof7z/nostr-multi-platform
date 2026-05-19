import AVFoundation
import Foundation
import MediaPlayer
import UIKit

extension PodcastPlayerStore {
    // MARK: - Position persistence

    func persistPosition() {
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

    func loadPositionRecord() -> PositionRecord? {
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

    func installTimeObserver(on player: AVPlayer) {
        let interval = CMTime(seconds: 0.25, preferredTimescale: 600)
        var lastPersistWall = Date.distantPast
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
                // Wall-clock-gated persistence: piggy-back on the existing
                // time observer rather than running a parallel polling task.
                if Date().timeIntervalSince(lastPersistWall) >= 5 {
                    lastPersistWall = Date()
                    self.persistPosition()
                }
            }
        }
    }

    func observeItem(_ item: AVPlayerItem) {
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

    func observeBuffering(_ item: AVPlayerItem) {
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

    func observeLoadedRanges(_ item: AVPlayerItem) {
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

    func observeError(_ item: AVPlayerItem) {
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

    func observePlaybackEnd(item: AVPlayerItem) {
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
    func configureRemoteCommandCenter() {
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

    func tearDownRemoteCommandCenter() {
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
    func updateNowPlayingInfo(artwork: MPMediaItemArtwork? = nil) {
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
    func fetchAndApplyArtwork(from urlString: String) {
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

    func tearDownPlayer() {
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
