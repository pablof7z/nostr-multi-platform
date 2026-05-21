import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// DmConversationView — one NIP-17 direct-message thread.
//
//   • Read:  the `DmConversation` for `peerPubkey` out of `DmInboxStore`'s
//            mirrored `nip17.dm_inbox` projection.
//   • Write: `nmp.dm.send` via `DmInboxStore.sendDm` — the kind:14 rumor,
//            the NIP-59 gift-wrap, and signing are all Rust-owned.
//
// Thin-shell rule: ZERO protocol logic here. The view re-derives nothing —
// the conversation list, ordering, and decrypted content all come from the
// Rust `DmInboxProjection`. The only Swift-side comparison is `senderPubkey
// == localPubkey` to align a bubble left vs right, which is presentation,
// not protocol.
// ─────────────────────────────────────────────────────────────────────────

struct DmConversationView: View {
    @ObservedObject var store: DmInboxStore
    /// The peer this thread is with (hex pubkey).
    let peerPubkey: String

    @State private var draft = ""
    @FocusState private var composerFocused: Bool

    private var trimmedDraft: String {
        draft.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// The live conversation for `peerPubkey`, re-resolved from the store on
    /// every render so new messages from the snapshot tick appear.
    private var conversation: DmConversation? {
        store.conversations.first { $0.peerPubkey == peerPubkey }
    }

    var body: some View {
        VStack(spacing: 0) {
            messageStream
            composer
        }
        .chirpScreenBackground()
        .navigationTitle(dmShortPubkey(peerPubkey))
        .navigationBarTitleDisplayMode(.inline)
    }

    // ── Message stream ────────────────────────────────────────────────────

    @ViewBuilder
    private var messageStream: some View {
        let messages = conversation?.messages ?? []
        if messages.isEmpty {
            ScrollView {
                ChirpPlaceholder(
                    systemImage: "bubble.left.and.bubble.right",
                    title: "No messages yet",
                    subtitle: "Send a private NIP-17 message to start the conversation."
                )
                .frame(minHeight: 320)
            }
        } else {
            ScrollView {
                LazyVStack(spacing: 8) {
                    // The projection emits newest-first; reverse for a chat
                    // log (oldest at top) — display ordering only, not a
                    // protocol decision.
                    ForEach(messages.reversed()) { message in
                        DmMessageBubble(
                            message: message,
                            isOutgoing: message.senderPubkey == store.localPubkey)
                    }
                }
                .padding(.vertical, 10)
                .padding(.horizontal, 12)
            }
        }
    }

    // ── Composer ──────────────────────────────────────────────────────────

    private var composer: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextEditor(text: $draft)
                .focused($composerFocused)
                .font(.body)
                .foregroundStyle(.primary)
                .frame(minHeight: 38, maxHeight: 120)
                .accessibilityIdentifier("dm-message-editor")
                .overlay(alignment: .topLeading) {
                    if draft.isEmpty {
                        Text("Message…")
                            .font(.body)
                            .foregroundStyle(.secondary)
                            .allowsHitTesting(false)
                            .padding(.top, 8)
                            .padding(.leading, 4)
                    }
                }

            Button {
                sendDraft()
            } label: {
                Image(systemName: "arrow.up.circle.fill")
                    .font(.system(size: 30))
                    .foregroundStyle(
                        trimmedDraft.isEmpty ? Color.secondary : Color.accentColor)
            }
            .buttonStyle(.plain)
            .disabled(trimmedDraft.isEmpty)
            .accessibilityLabel("Send")
            .accessibilityIdentifier("dm-send-button")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(ChirpColor.bg)
        .overlay(alignment: .top) { Divider() }
    }

    private func sendDraft() {
        let text = trimmedDraft
        guard !text.isEmpty else { return }
        // Fire-and-forget: the sent message reappears via the next snapshot
        // tick (the actor gift-wraps a self-copy to the sender). Clearing the
        // draft optimistically matches `GroupChatView` / `ComposeView`.
        store.sendDm(to: peerPubkey, content: text)
        draft = ""
    }
}

// ── Message bubble ────────────────────────────────────────────────────────

private struct DmMessageBubble: View {
    let message: DmMessage
    /// `true` when the local account wrote this message (right-aligned).
    let isOutgoing: Bool

    var body: some View {
        HStack {
            if isOutgoing { Spacer(minLength: 48) }
            VStack(alignment: isOutgoing ? .trailing : .leading, spacing: 2) {
                Text(message.content)
                    .font(.body)
                    .foregroundStyle(isOutgoing ? Color.white : Color.primary)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(
                        isOutgoing ? Color.accentColor : Color(.secondarySystemBackground),
                        in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                Text(dmRelativeTime(message.createdAt))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            if !isOutgoing { Spacer(minLength: 48) }
        }
        .accessibilityIdentifier("dm-message-\(message.id)")
    }
}
