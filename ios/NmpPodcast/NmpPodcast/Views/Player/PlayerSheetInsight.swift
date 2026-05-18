import SwiftUI
import AVFoundation
import OSLog

extension PlayerSheet {
    // MARK: - Gestures

    func dismissGesture(height: CGFloat) -> some Gesture {
        DragGesture()
            .onChanged { value in
                if value.translation.height > 0 {
                    dragOffset = value.translation.height
                }
            }
            .onEnded { value in
                if value.translation.height > height * 0.25 || value.predictedEndTranslation.height > height * 0.5 {
                    dismiss()
                } else {
                    withAnimation(.spring(response: 0.3)) {
                        dragOffset = 0
                    }
                }
            }
    }

    // MARK: - Helpers

    var playPauseIcon: String {
        switch audioService.playbackState {
        case .playing:
            return "pause.fill"
        case .loading:
            return "ellipsis"
        case .error:
            return "exclamationmark.triangle.fill"
        default:
            return "play.fill"
        }
    }

    func togglePlayback() {
        switch audioService.playbackState {
        case .playing:
            audioService.pause()
        case .paused:
            audioService.resume()
        case .idle, .error:
            // Retry playback from current position
            if let episode = audioService.currentEpisode {
                Task {
                    let position = audioService.currentTime > 0 ? audioService.currentTime : nil
                    await audioService.play(episode: episode, from: position)
                }
            }
        case .loading:
            break
        }
    }

    static var speedPresets: [Float] { [0.5, 1.0, 1.25, 1.5, 2.0] }

    func cyclePlaybackSpeed() {
        let currentRate = audioService.playbackRate
        let currentIndex = Self.speedPresets.firstIndex(of: currentRate) ?? 1
        let nextIndex = (currentIndex + 1) % Self.speedPresets.count
        audioService.setPlaybackRate(Self.speedPresets[nextIndex])
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }

    func formatSpeed(_ rate: Float) -> String {
        if rate == Float(Int(rate)) {
            return "\(Int(rate))×"
        } else {
            return String(format: "%.2g×", rate)
        }
    }

    func formatTime(_ time: TimeInterval) -> String {
        guard time.isFinite && !time.isNaN else { return "0:00" }
        let totalSeconds = Int(time)
        let hours = totalSeconds / 3600
        let minutes = (totalSeconds % 3600) / 60
        let seconds = totalSeconds % 60

        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, seconds)
        } else {
            return String(format: "%d:%02d", minutes, seconds)
        }
    }

    // MARK: - Insight Capture

    func startInsightCapture(at time: TimeInterval) {
        captureTime = time
        audioService.pause()
        startCapture()
    }

    func startCapture() {
        guard audioService.currentEpisode != nil else { return }

        captureTime = audioService.currentTime
        audioService.pause()

        Task {
            do {
                let session = AVAudioSession.sharedInstance()
                try session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker, .allowBluetoothA2DP])
                try session.setActive(true)

                _ = try insightService.startRecording()
                isCapturing = true
            } catch {
                Logger.player.error("Failed to start recording: \(error)")
                audioService.resume()
            }
        }
    }

    func stopCapture() {
        guard let episode = audioService.currentEpisode else { return }

        isCapturing = false

        guard let audioURL = insightService.stopRecording() else {
            audioService.resume()
            return
        }

        Task {
            do {
                let session = AVAudioSession.sharedInstance()
                try session.setCategory(.playback, mode: .spokenAudio, options: [])
                try session.setActive(true)
            } catch {
                Logger.audio.error("Failed to reset audio session: \(error)")
            }

            audioService.resume()
            processInsightInBackground(audioURL: audioURL, episode: episode)
        }
    }

    func processInsightInBackground(audioURL: URL, episode: Episode) {
        Task {
            do {
                _ = try await insightService.processInsight(
                    audioURL: audioURL,
                    episode: episode,
                    captureTime: captureTime,
                    modelContext: modelContext
                )

                await MainActor.run {
                    withAnimation {
                        showInsightSavedToast = true
                    }

                    Task {
                        try? await Task.sleep(nanoseconds: 3_000_000_000)
                        withAnimation {
                            showInsightSavedToast = false
                        }
                    }
                }
            } catch {
                Logger.ai.error("Failed to process insight: \(error)")
                await MainActor.run {
                    insightErrorMessage = error.localizedDescription
                    withAnimation {
                        showInsightErrorToast = true
                    }

                    Task {
                        try? await Task.sleep(nanoseconds: 4_000_000_000)
                        withAnimation {
                            showInsightErrorToast = false
                        }
                    }
                }
            }
        }
    }
}
