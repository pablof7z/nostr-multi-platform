import SwiftUI

// MARK: - NostrConversationDetailView
//
// T-podcast-gap-002: Stub for Nostr conversation deep-link destination.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Settings/Agent/NostrConversationDetailView.swift

struct NostrConversationDetailView: View {
    let conversation: NostrConversation

    var body: some View {
        ContentUnavailableView(
            "Conversation",
            systemImage: "bubble.left.and.bubble.right",
            description: Text("Nostr conversation loads once the kernel exposes nostr events (T-podcast-gap-002).")
        )
        .navigationTitle("Conversation")
    }
}
