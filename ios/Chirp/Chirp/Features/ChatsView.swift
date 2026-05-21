import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// ChatsView — Chats tab root.
//
// Thin wrapper that promotes DmListView to a tab root, reading the shared
// dmInbox store from the environment. All DM logic, state, and ordering
// live in Rust via DmInboxStore; this view only wires the environment.
// ─────────────────────────────────────────────────────────────────────────

struct ChatsView: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        DmListView(store: model.dmInbox)
    }
}
