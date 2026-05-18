import SwiftUI

// MARK: - EpisodeDetailView
//
// T-podcast-gap-002: Verbatim Podcastr EpisodeDetailView requires full episode
// data including chapters, transcripts, and clips. Stub until kernel exposes
// these.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/EpisodeDetail/EpisodeDetailView.swift

struct EpisodeDetailView: View {
    let episodeID: UUID

    var body: some View {
        ContentUnavailableView(
            "Episode",
            systemImage: "headphones",
            description: Text("Episode detail loads once the kernel exposes episode data (T-podcast-gap-002).")
        )
        .navigationTitle("Episode")
    }
}

/// Placeholder for LibraryEpisodeRoute navigation destination.
struct LibraryEpisodePlaceholder: View {
    let route: LibraryEpisodeRoute

    var body: some View {
        EpisodeDetailView(episodeID: route.episodeID)
    }
}
