import SwiftUI

// MARK: - ShowDetailEpisodeList

/// The episode list inside `ShowDetailView`. Extracted into its own file to
/// keep `ShowDetailView` under the 300-line soft limit and to give the row
/// surface (which now wires context-menu + swipe-actions + accessibility-actions)
/// somewhere coherent to live.
///
/// Renders a `ForEach` of `EpisodeRow`s — the parent decides whether to host
/// them inside a `List` (preferred, so swipe actions activate) or any other
/// container.
struct ShowDetailEpisodeList: View {
    let podcast: Podcast
    let episodes: [Episode]

    @Environment(AppStateStore.self) private var store
    @Environment(PlaybackState.self) private var playback

    /// Selection driven by the VoiceOver "Open episode details" custom action —
    /// `accessibilityActions` cannot host a `NavigationLink`, so we route through
    /// `.navigationDestination(item:)` on the parent view.
    @Binding var voiceOverDetailRoute: LibraryEpisodeRoute?

    var body: some View {
        ForEach(episodes) { ep in
            NavigationLink(value: LibraryEpisodeRoute(
                episodeID: ep.id,
                subscriptionID: podcast.id,
                title: ep.title
            )) {
                EpisodeRow(
                    episode: ep,
                    showAccent: podcast.accentColor,
                    fallbackImageURL: podcast.imageURL,
                    onPlay: {
                        playback.setEpisode(ep)
                        playback.play()
                        NotificationCenter.default.post(name: .openPlayerRequested, object: nil)
                    }
                )
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .listRowSeparator(.hidden)
            .listRowInsets(EdgeInsets(
                top: AppTheme.Spacing.xs,
                leading: AppTheme.Spacing.lg,
                bottom: AppTheme.Spacing.xs,
                trailing: AppTheme.Spacing.lg
            ))
            .listRowBackground(Color(.systemBackground))
            .swipeActions(edge: .leading, allowsFullSwipe: true) {
                EpisodeRowLeadingSwipeAction(episode: ep, playback: playback)
            }
            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                EpisodeRowTrailingSwipeAction(episode: ep, store: store)
                EpisodeRowDownloadSwipeAction(episode: ep, store: store)
            }
            .contextMenu {
                EpisodeRowContextMenu(
                    episode: ep,
                    store: store,
                    openDetailsRoute: LibraryEpisodeRoute(
                        episodeID: ep.id,
                        subscriptionID: podcast.id,
                        title: ep.title
                    ),
                    playback: playback
                )
            }
            .accessibilityActions {
                EpisodeRowAccessibilityActions(
                    episode: ep,
                    store: store,
                    onOpenDetails: {
                        voiceOverDetailRoute = LibraryEpisodeRoute(
                            episodeID: ep.id,
                            subscriptionID: podcast.id,
                            title: ep.title
                        )
                    }
                )
            }
        }
    }
}
