import SwiftUI
import UIKit

struct MiniPlayer: View {
    @Bindable var audioService: AudioService
    var onTap: () -> Void
    var onClose: () -> Void

    @State private var isDragging = false
    @State private var dragProgress: Double = 0
    @GestureState private var dragOffset: CGFloat = 0

    var body: some View {
        if let episode = audioService.currentEpisode {
            VStack(spacing: 0) {
                // Scrubbing progress bar
                GeometryReader { geometry in
                    ZStack(alignment: .leading) {
                        // Track
                        RoundedRectangle(cornerRadius: 1.5)
                            .fill(Color.primary.opacity(0.15))

                        // Progress
                        RoundedRectangle(cornerRadius: 1.5)
                            .fill(Color.accentColor)
                            .frame(width: geometry.size.width * displayProgress)
                    }
                    .gesture(
                        DragGesture(minimumDistance: 0)
                            .onChanged { value in
                                if !isDragging {
                                    isDragging = true
                                    UIImpactFeedbackGenerator(style: .light).impactOccurred()
                                }
                                let progress = max(0, min(1, value.location.x / geometry.size.width))
                                dragProgress = progress
                            }
                            .onEnded { value in
                                let progress = max(0, min(1, value.location.x / geometry.size.width))
                                audioService.seek(to: progress * audioService.duration)
                                isDragging = false
                                UIImpactFeedbackGenerator(style: .light).impactOccurred()
                            }
                    )
                }
                .frame(height: 3)

                HStack(spacing: 12) {
                    // Artwork thumbnail
                    CachedAsyncImage(url: episode.podcast?.artworkURL) {
                        RoundedRectangle(cornerRadius: 6)
                            .fill(Color.secondary.opacity(0.2))
                            .overlay {
                                Image(systemName: "waveform")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                    }
                    .aspectRatio(contentMode: .fill)
                    .frame(width: 44, height: 44)
                    .clipShape(RoundedRectangle(cornerRadius: 6))

                    // Episode info
                    VStack(alignment: .leading, spacing: 2) {
                        Text(episode.title)
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .lineLimit(1)

                        if let podcastTitle = episode.podcast?.title {
                            Text(podcastTitle)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }

                    Spacer()

                    // Playback controls
                    HStack(spacing: 16) {
                        Button {
                            UIImpactFeedbackGenerator(style: .light).impactOccurred()
                            audioService.skipBackward(15)
                        } label: {
                            Image(systemName: "gobackward.15")
                                .font(.title3)
                        }

                        Button {
                            UIImpactFeedbackGenerator(style: .medium).impactOccurred()
                            togglePlayback()
                        } label: {
                            Image(systemName: playPauseIcon)
                                .font(.title2)
                                .frame(width: 32, height: 32)
                        }
                        .accessibilityIdentifier("playPauseButton")

                        Button {
                            UIImpactFeedbackGenerator(style: .light).impactOccurred()
                            audioService.skipForward(30)
                        } label: {
                            Image(systemName: "goforward.30")
                                .font(.title3)
                        }
                    }
                    .foregroundStyle(.primary)

                    Button {
                        onClose()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.title3)
                            .foregroundStyle(.tertiary)
                    }
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
            }
            .background(.ultraThinMaterial)
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .shadow(color: .black.opacity(0.1), radius: 8, x: 0, y: -2)
            .padding(.horizontal, 8)
            .contentShape(Rectangle())
            .onTapGesture(perform: onTap)
        }
    }

    private var displayProgress: Double {
        isDragging ? dragProgress : progress
    }

    private var progress: Double {
        guard audioService.duration > 0 else { return 0 }
        return audioService.currentTime / audioService.duration
    }

    private var playPauseIcon: String {
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

    private func togglePlayback() {
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
}
