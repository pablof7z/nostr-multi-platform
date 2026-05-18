import SwiftUI

// MARK: - NostrConversationsToolbarItem
//
// T-podcast-gap-002: Stub toolbar item. Verbatim Podcastr version requires
// NostrConversation data from the kernel.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Settings/Agent/NostrConversationsView.swift

struct NostrConversationsToolbarItem: ToolbarContent {
    var body: some ToolbarContent {
        // T-podcast-gap-002: Nostr conversation badge lands when kernel exposes nostr events.
        ToolbarItem(placement: .topBarTrailing) {
            EmptyView()
        }
    }
}
