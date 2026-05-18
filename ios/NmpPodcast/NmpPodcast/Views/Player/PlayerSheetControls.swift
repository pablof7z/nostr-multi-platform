import SwiftUI
import UIKit

extension PlayerSheet {
    // MARK: - Controls Bar

    func controlsBar(episode: Episode) -> some View {
        VStack(spacing: 8) {
            // Progress scrubber
            scrubber

            // Controls
            HStack(spacing: 0) {
                // Speed
                Button {
                    cyclePlaybackSpeed()
                } label: {
                    Text(formatSpeed(audioService.playbackRate))
                        .font(.caption)
                        .fontWeight(.semibold)
                        .frame(width: 44, height: 44)
                }
                .foregroundStyle(.secondary)

                Spacer()

                // Main controls
                HStack(spacing: 32) {
                    Button {
                        UIImpactFeedbackGenerator(style: .light).impactOccurred()
                        audioService.skipBackward(15)
                    } label: {
                        Image(systemName: "gobackward.15")
                            .font(.title2)
                    }

                    Button {
                        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
                        togglePlayback()
                    } label: {
                        ZStack {
                            Circle()
                                .fill(Color.accentColor)
                                .frame(width: 64, height: 64)

                            Image(systemName: playPauseIcon)
                                .font(.title)
                                .foregroundStyle(.white)
                        }
                    }
                    .accessibilityIdentifier("playPauseButton")

                    Button {
                        UIImpactFeedbackGenerator(style: .light).impactOccurred()
                        audioService.skipForward(30)
                    } label: {
                        Image(systemName: "goforward.30")
                            .font(.title2)
                    }
                }
                .foregroundStyle(.primary)

                Spacer()

                // Capture insight
                captureButton
                    .frame(width: 44, height: 44)
            }
            .padding(.horizontal)
        }
        .padding(.vertical, 12)
        .background(.ultraThinMaterial)
    }

    var scrubber: some View {
        VStack(spacing: 4) {
            Slider(
                value: Binding(
                    get: { audioService.currentTime },
                    set: { audioService.seek(to: $0) }
                ),
                in: 0...max(audioService.duration, 1)
            )
            .tint(.accentColor)

            HStack {
                Text(formatTime(audioService.currentTime))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()

                Spacer()

                Text("-" + formatTime(audioService.duration - audioService.currentTime))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
        }
        .padding(.horizontal)
    }

    // MARK: - Capture Button

    var captureButton: some View {
        Button {
            if isCapturing {
                stopCapture()
            } else {
                startCapture()
            }
        } label: {
            ZStack {
                Circle()
                    .fill(isCapturing ? Color.red : Color.clear)
                    .scaleEffect(pulseAnimation && isCapturing ? 1.2 : 1.0)
                    .opacity(pulseAnimation && isCapturing ? 0.5 : 1.0)

                Image(systemName: isCapturing ? "stop.fill" : "lightbulb")
                    .font(.body)
                    .foregroundStyle(isCapturing ? .white : .secondary)
            }
        }
        .accessibilityLabel(isCapturing ? "Stop recording insight" : "Capture insight")
        .accessibilityIdentifier("captureInsightButton")
        .onChange(of: isCapturing) { _, newValue in
            if newValue {
                withAnimation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true)) {
                    pulseAnimation = true
                }
            } else {
                withAnimation(.default) {
                    pulseAnimation = false
                }
            }
        }
    }

    // MARK: - Toast Overlays

    var toastOverlays: some View {
        Group {
            if showInsightSavedToast {
                InsightSavedToast()
                    .transition(.move(edge: .top).combined(with: .opacity))
                    .padding(.top, 60)
            }
            if showInsightErrorToast {
                InsightErrorToast(message: insightErrorMessage)
                    .transition(.move(edge: .top).combined(with: .opacity))
                    .padding(.top, 60)
            }
        }
    }
}
