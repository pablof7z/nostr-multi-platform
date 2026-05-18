import Kingfisher
import SwiftUI

struct MiniPlayerView: View {
    @Environment(HighlighterStore.self) private var app
    @Environment(\.tabViewBottomAccessoryPlacement) private var placement
    @Namespace private var heroNamespace
    @State private var playerSheetPresented = false

    private var player: PodcastPlayerStore { app.podcastPlayer }

    var body: some View {
        guard let artifact = player.currentArtifact else { return AnyView(EmptyView()) }
        return AnyView(content(artifact: artifact))
    }

    @ViewBuilder
    private func content(artifact: ArtifactRecord) -> some View {
        Group {
            if placement == .inline {
                inlineRow(artifact: artifact)
            } else {
                expandedRow(artifact: artifact)
            }
        }
        .overlay(alignment: .bottom) {
            progressBar
                .padding(.horizontal, 4)
                .padding(.bottom, 3)
        }
        .contentShape(.capsule)
        .onTapGesture { playerSheetPresented = true }
        .contextMenu {
            Button {
                player.skip(by: 30)
            } label: {
                Label("Skip 30 seconds", systemImage: "goforward.30")
            }

            Button {
                let end = player.currentTime
                let start = max(0, end - 60)
                player.setClipStart(start)
                player.setClipEnd(end)
            } label: {
                Label("Mark clip", systemImage: "scissors")
            }

            Button(role: .destructive) {
                player.clear()
            } label: {
                Label("Stop", systemImage: "stop.fill")
            }
        }
        .sheet(isPresented: $playerSheetPresented) {
            PodcastListeningView(heroSourceID: "podcast-mini-art", heroNamespace: heroNamespace)
                .environment(app)
                .presentationDetents([.large])
                .navigationTransition(.zoom(sourceID: "podcast-mini-art", in: heroNamespace))
        }
        .animation(.snappy, value: placement)
    }

    @ViewBuilder
    private func expandedRow(artifact: ArtifactRecord) -> some View {
        HStack(spacing: 12) {
            artwork(artifact: artifact, size: 40, cornerRadius: 6)
                .matchedTransitionSource(id: "podcast-mini-art", in: heroNamespace)

            VStack(alignment: .leading, spacing: 2) {
                let showTitle = artifact.preview.podcastShowTitle.isEmpty
                    ? artifact.preview.author
                    : artifact.preview.podcastShowTitle
                if !showTitle.isEmpty {
                    Text(showTitle)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Text(artifact.preview.title.isEmpty ? "Untitled episode" : artifact.preview.title)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            playPauseButton(size: 36)
        }
        .padding(.horizontal, 12)
        .frame(height: 56)
    }

    @ViewBuilder
    private func inlineRow(artifact: ArtifactRecord) -> some View {
        HStack(spacing: 8) {
            artwork(artifact: artifact, size: 24, cornerRadius: 4)
                .matchedTransitionSource(id: "podcast-mini-art", in: heroNamespace)

            Text(artifact.preview.title.isEmpty ? "Untitled episode" : artifact.preview.title)
                .font(.footnote.weight(.semibold))
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)

            playPauseButton(size: 28)
        }
        .padding(.horizontal, 10)
    }

    private func playPauseButton(size: CGFloat) -> some View {
        Button {
            player.toggle()
        } label: {
            ZStack {
                if player.isBuffering {
                    ProgressView()
                        .controlSize(.small)
                        .frame(width: size, height: size)
                } else {
                    Image(systemName: player.isPlaying ? "pause.fill" : "play.fill")
                        .font(.system(size: size * 0.45, weight: .semibold))
                        .frame(width: size, height: size)
                }
            }
        }
        .buttonStyle(.plain)
    }

    @ViewBuilder
    private func artwork(artifact: ArtifactRecord, size: CGFloat, cornerRadius: CGFloat) -> some View {
        let imageUrl = artifact.preview.image
        Group {
            if !imageUrl.isEmpty, let url = URL(string: imageUrl) {
                KFImage(url)
                    .placeholder { artworkPlaceholder }
                    .fade(duration: 0.15)
                    .resizable()
                    .scaledToFill()
            } else {
                artworkPlaceholder
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
    }

    private var artworkPlaceholder: some View {
        ZStack {
            Color.secondary.opacity(0.2)
            Image(systemName: "waveform")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    private var progressBar: some View {
        GeometryReader { geo in
            let fraction: Double = player.duration > 0
                ? min(1, max(0, player.currentTime / player.duration))
                : 0
            Rectangle()
                .fill(.primary.opacity(0.6))
                .frame(width: geo.size.width * fraction, height: 1)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(height: 1)
    }
}
