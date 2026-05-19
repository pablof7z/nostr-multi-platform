import SwiftUI

// MARK: - WikiView
//
// T-podcast-gap-002: Verbatim Podcastr WikiView requires WikiStorage and
// WikiPage model data from the kernel. Renders empty state honestly until
// kernel exposes wiki data.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Wiki/WikiView.swift

struct WikiView: View {
    var body: some View {
        NavigationStack {
            ContentUnavailableView(
                "Wiki",
                systemImage: "book.closed.fill",
                description: Text("Your knowledge wiki appears here. Loads once the kernel exposes wiki data (T-podcast-gap-002).")
            )
            .navigationTitle("Wiki")
            .navigationBarTitleDisplayMode(.inline)
        }
    }
}
