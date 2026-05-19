import SwiftUI

// MARK: - HomeView
//
// T-podcast-gap-002: Verbatim Podcastr HomeView is not yet compiled because
// its dependency closure (AppStateStore, PlaybackState, InboxTriageService,
// ThreadingInferenceService, Episode, Podcast, etc.) requires the full kernel
// data model to be backed. Renders Podcastr's empty state honestly until the
// kernel FeedViewModule lands.
//
// When the kernel exposes episode + subscription data, replace this file with
// a byte-for-byte copy from:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Home/HomeView.swift

struct HomeView: View {
    var body: some View {
        ContentUnavailableView(
            "Home",
            systemImage: "house.fill",
            description: Text("Your listening home. Loads once the kernel exposes episode data (T-podcast-gap-002).")
        )
    }
}
