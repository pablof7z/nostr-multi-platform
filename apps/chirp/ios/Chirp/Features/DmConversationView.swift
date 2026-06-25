import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// DmConversationView — one NIP-17 direct-message thread.
//
//   • Read:  the `DmConversation` for `peerPubkey` out of `DmInboxStore`'s
//            mirrored `nip17.dm_inbox` projection.
//   • Write: `nmp.nip17.send` via `DmInboxStore.sendDm` — the kind:14 rumor,
//            the NIP-59 gift-wrap, and signing are all Rust-owned.
//
// Thin-shell rule: ZERO protocol logic here. The conversation list,
// chronological ordering, decrypted content, and per-message `isOutgoing`
// flag all come from the Rust `DmInboxProjection`. The title is rendered from
// the Rust-owned profile projection through `NostrProfileName`.
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
        .navigationTitle(peerPubkey.shortHex)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .principal) {
                NostrProfileName(
                    pubkey: peerPubkey,
                    font: .headline,
                    color: .primary,
                    consumerID: "dm-conversation.title.\(peerPubkey)")
            }
        }
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
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 8) {
                        // The projection emits messages in chronological
                        // order (oldest first, newest last). No reverse here
                        // — that decision lives in Rust.
                        ForEach(messages) { message in
                            DmMessageBubble(message: message)
                        }
                        ChirpColor.transparent.frame(height: 1).id("dm-bottom")
                    }
                    .padding(.vertical, 10)
                    .padding(.horizontal, 12)
                }
                .onChange(of: messages.count) { _, _ in
                    proxy.scrollTo("dm-bottom")
                }
                .onAppear {
                    // Pure UI animation timing: the ScrollViewReader needs
                    // its first layout pass before `scrollTo` resolves. Not
                    // a polling loop — no state is being awaited.
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                        proxy.scrollTo("dm-bottom")
                    }
                }
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
                        trimmedDraft.isEmpty ? ChirpColor.textSecondary : ChirpColor.accent)
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
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        draft = ""
    }
}

// ── Message bubble ────────────────────────────────────────────────────────

private struct DmMessageBubble: View {
    let message: DmMessage

    var body: some View {
        let outgoing = message.isOutgoing
        HStack {
            if outgoing { Spacer(minLength: 48) }
            VStack(alignment: outgoing ? .trailing : .leading, spacing: 2) {
                Text(message.content)
                    .font(.body)
                    .foregroundStyle(
                        outgoing ? ChirpColor.messageOutgoingForeground : ChirpColor.messageIncomingForeground)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(
                        outgoing ? ChirpColor.messageOutgoingBackground : ChirpColor.messageIncomingBackground,
                        in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                Text(message.createdAt.relativeTimeFromUnixSeconds)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            if !outgoing { Spacer(minLength: 48) }
        }
        .accessibilityIdentifier("dm-message-\(message.id)")
    }
}

/// Presentation helpers for DM peer labels backed by Rust-owned profile data.
enum DmPeerPresentation {
    static func label(pubkey: String, profileDisplay: String?) -> String {
        if let profileDisplay, !profileDisplay.isEmpty {
            return profileDisplay
        }
        return pubkey.shortHex
    }

    static func matchesContact(
        pubkey: String,
        profileDisplay: String?,
        query: String
    ) -> Bool {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !needle.isEmpty else { return true }
        if pubkey.lowercased().contains(needle) { return true }
        return label(pubkey: pubkey, profileDisplay: profileDisplay)
            .lowercased()
            .contains(needle)
    }
}
