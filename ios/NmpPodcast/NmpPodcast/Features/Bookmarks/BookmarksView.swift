import SwiftUI

// MARK: - BookmarksView
//
// T-podcast-gap-002: Verbatim Podcastr BookmarksView requires Episode + Clip
// data from kernel. Renders empty state honestly until kernel exposes episode
// data.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Bookmarks/BookmarksView.swift

struct BookmarksView: View {
    var body: some View {
        ContentUnavailableView(
            "Bookmarks",
            systemImage: "bookmark.fill",
            description: Text("Starred and clipped episodes appear here. Loads once the kernel exposes episode data (T-podcast-gap-002).")
        )
    }
}
