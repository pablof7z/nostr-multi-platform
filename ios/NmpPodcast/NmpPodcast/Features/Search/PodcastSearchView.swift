import SwiftUI

// MARK: - PodcastSearchView
//
// T-podcast-gap-003: Verbatim Podcastr PodcastSearchView requires Podcast
// Index API and SubscriptionService. Stub until those land.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Search/PodcastSearchView.swift

struct PodcastSearchView: View {
    var body: some View {
        ContentUnavailableView(
            "Search",
            systemImage: "magnifyingglass",
            description: Text("Podcast search loads once the kernel exposes feed discovery (T-podcast-gap-003).")
        )
        .navigationTitle("Search")
        .navigationBarTitleDisplayMode(.inline)
    }
}
