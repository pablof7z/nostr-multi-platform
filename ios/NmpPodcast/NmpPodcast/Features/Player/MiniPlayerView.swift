import SwiftUI

// MARK: - MiniPlayerView
//
// T-podcast-gap-002: Verbatim Podcastr MiniPlayerView requires PlaybackState.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Player/MiniPlayerView.swift

struct MiniPlayerView: View {
    let state: PlaybackState
    let onTap: () -> Void
    var glassNamespace: Namespace.ID

    var body: some View {
        Button(action: onTap) {
            HStack {
                if let episode = state.episode {
                    Text(episode.title)
                        .font(AppTheme.Typography.caption)
                        .lineLimit(1)
                }
                Spacer()
                Image(systemName: "pause.circle.fill")
                    .font(.title2)
            }
            .padding(.horizontal, AppTheme.Spacing.md)
            .padding(.vertical, AppTheme.Spacing.sm)
            .background(.thinMaterial)
        }
        .buttonStyle(.plain)
    }
}
