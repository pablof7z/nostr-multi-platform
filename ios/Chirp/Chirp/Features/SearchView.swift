import SwiftUI
// OWNER: Phase-2 Agent D (Search — polished stub; FFI has no search yet,
// so a tasteful "coming in Chirp CX4" surface + npub/hashtag open box).
struct SearchView: View {
    @EnvironmentObject private var model: KernelModel
    var body: some View {
        ChirpPlaceholder(systemImage: "magnifyingglass", title: "Search",
            subtitle: "Agent D builds this.")
            .navigationTitle("Search")
    }
}
