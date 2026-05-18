import SwiftUI
// OWNER: Phase-2 Agent B (Thread screen). Replace whole file.
// NOTE: type is ThreadScreen (the bridge already defines a Decodable
// `ThreadView` model — do not name a View `ThreadView`).
// Init signature is FIXED by the nav contract: ThreadScreen(eventID:).
struct ThreadScreen: View {
    let eventID: String
    @EnvironmentObject private var model: KernelModel
    var body: some View {
        ChirpPlaceholder(systemImage: "bubble.left.and.bubble.right",
            title: "Thread", subtitle: "Agent B builds this.")
            .navigationTitle("Thread").navigationBarTitleDisplayMode(.inline)
    }
}
