import SwiftUI

// MARK: - LibraryEpisodeRoute

/// Navigation value pushed onto the show-detail `NavigationStack` when the
/// user taps an episode row. Encapsulating the route as a value type (rather
/// than a hard `NavigationLink(destination:)`) keeps Library's row surface
/// decoupled from `EpisodeDetailView`'s internal init.
struct LibraryEpisodeRoute: Hashable {
    let episodeID: UUID
    let subscriptionID: UUID
    let title: String
}

// MARK: - EpisodeDetailLink

/// Tap-row → push-route helper. A button shaped like a list cell content
/// container that pushes a `LibraryEpisodeRoute` onto the enclosing
/// `NavigationStack`.
struct EpisodeDetailLink<Label: View>: View {
    let route: LibraryEpisodeRoute
    @ViewBuilder let label: () -> Label

    var body: some View {
        NavigationLink(value: route) {
            label()
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

// MARK: - LibraryEpisodePlaceholder

/// Bridge between Library's `LibraryEpisodeRoute` and the real
/// `EpisodeDetailView`. Library still holds the `navigationDestination`
/// for `LibraryEpisodeRoute` and renders this view; this view forwards
/// into the real episode-detail surface using the route's `episodeID`.
struct LibraryEpisodePlaceholder: View {
    let route: LibraryEpisodeRoute

    var body: some View {
        EpisodeDetailView(episodeID: route.episodeID)
    }
}
