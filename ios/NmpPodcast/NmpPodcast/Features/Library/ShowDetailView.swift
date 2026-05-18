import SwiftUI

// MARK: - ShowDetailView
//
// T-podcast-gap-002: Verbatim Podcastr ShowDetailView requires full episode
// list and subscription management. Stub until kernel exposes episode data.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Library/ShowDetailView.swift

struct ShowDetailView: View {
    let podcast: Podcast

    var body: some View {
        ContentUnavailableView(
            podcast.title,
            systemImage: "tray.full",
            description: Text("Podcast episode list loads once the kernel exposes episode data (T-podcast-gap-002).")
        )
        .navigationTitle(podcast.title)
        .navigationBarTitleDisplayMode(.large)
    }
}
