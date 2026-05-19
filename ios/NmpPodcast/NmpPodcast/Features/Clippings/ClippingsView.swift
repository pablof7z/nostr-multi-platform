import SwiftUI

// MARK: - ClippingsView
//
// T-podcast-gap-002: Verbatim Podcastr ClippingsView requires Clip data
// from the kernel. Renders empty state honestly until kernel exposes clip
// data.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Clippings/ClippingsView.swift

struct ClippingsView: View {
    var body: some View {
        ContentUnavailableView(
            "Clippings",
            systemImage: "scissors",
            description: Text("Your audio clips appear here. Loads once the kernel exposes clip data (T-podcast-gap-002).")
        )
    }
}
