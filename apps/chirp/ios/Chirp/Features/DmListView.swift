import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// DmListView — the NIP-17 private direct-message inbox.
//
// First consumer of the NIP-17 receive seam:
//   • Read:  `projections["nmp.nip17.dm_inbox"]`, mirrored by `DmInboxStore`
//            (registered via `nmp_app_chirp_register_dm_inbox`).
//   • Write: `nmp.nip17.send` via `KernelHandle.sendDm` — reached from
//            `DmConversationView`.
//
// Thin-shell rule: ZERO protocol logic here. Conversations arrive
// newest-thread-first from the Rust `DmInboxProjection`; profile labels and
// images come from the Rust-owned profile projections through `KernelModel`.
// Swift only renders the list, navigates into a thread, and falls back to the
// existing presentation-only short key when no profile is available.
// ─────────────────────────────────────────────────────────────────────────

struct DmListView: View {
    @ObservedObject var store: DmInboxStore
    @EnvironmentObject private var model: KernelModel

    @State private var showCompose = false

    var body: some View {
        Group {
            if store.isUnavailable {
                // §D7 "unavailable": no active account — host hides the DM screen.
                unavailableState
            } else if store.conversations.isEmpty && !store.isLimited {
                emptyState
            } else {
                // §D7 "limited" renders the list WITH a "still decrypting" banner
                // (errors-as-state) rather than hiding pending messages.
                conversationList
            }
        }
        .chirpScreenBackground()
        .navigationTitle("Chats")
        .navigationBarTitleDisplayMode(.large)
        .toolbar {
            ToolbarItem(placement: .navigationBarTrailing) {
                Button {
                    showCompose = true
                } label: {
                    Image(systemName: "square.and.pencil")
                        .font(.system(size: 17, weight: .semibold))
                }
                .accessibilityLabel("New message")
                .accessibilityIdentifier("dm-new-message-button")
                // Only disabled with no active account (§D7 "unavailable"); a
                // bunker account CAN now send/decrypt (ADR-0050 §D6).
                .disabled(store.isUnavailable)
            }
        }
        .sheet(isPresented: $showCompose) {
            DmComposeSheet(store: store)
                .environmentObject(model)
        }
    }

    private var emptyState: some View {
        ScrollView {
            ChirpPlaceholder(
                systemImage: "lock.fill",
                title: "No chats yet",
                subtitle: "Your chats are private and end-to-end encrypted."
            )
            .frame(minHeight: 360)
        }
    }

    private var unavailableState: some View {
        ScrollView {
            ChirpPlaceholder(
                systemImage: "exclamationmark.lock.fill",
                title: "DMs unavailable",
                subtitle: "Sign in to an account to send and read end-to-end encrypted messages."
            )
            .frame(minHeight: 360)
        }
    }

    /// ADR-0050 §D7 "limited" banner — a bunker backfill is pending or throttled
    /// by the bounded per-account decrypt queue. Surfaced as state (the count is
    /// never silently dropped), shown above whatever has already decrypted.
    private var decryptingBanner: some View {
        HStack(spacing: 8) {
            ProgressView()
                .controlSize(.small)
            Text("^[\(Int(store.undecryptedCount)) message](inflect: true) still decrypting…")
                .font(.footnote)
                .foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .accessibilityIdentifier("dm-decrypting-banner")
    }

    private var conversationList: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if store.isLimited && store.undecryptedCount > 0 {
                    decryptingBanner
                }
                // The projection emits conversations newest-thread-first;
                // render in that order — no Swift-side re-sort (thin-shell).
                ForEach(store.conversations) { conversation in
                    NavigationLink {
                        DmConversationView(store: store, peerPubkey: conversation.peerPubkey)
                    } label: {
                        DmConversationRow(conversation: conversation)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.vertical, 4)
        }
    }
}

// ── Conversation row ──────────────────────────────────────────────────────

private struct DmConversationRow: View {
    @EnvironmentObject private var model: KernelModel

    let conversation: DmConversation

    /// The most recent message — the last entry, since the projection
    /// orders each thread chronologically (oldest first, newest last).
    private var latest: DmMessage? { conversation.messages.last }

    private var peerDisplayLabel: String {
        DmPeerPresentation.label(
            pubkey: conversation.peerPubkey,
            profileDisplay: model.profile(forPubkey: conversation.peerPubkey)?.display)
    }

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            // The avatar self-claims the peer's kind:0. Its picture comes
            // from the Rust-owned profile projection; identicon fallback is
            // deterministic from the pubkey.
            NostrAvatar(
                pubkey: conversation.peerPubkey,
                url: nil,
                size: 40
            )
            .equatable()

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    NostrProfileName(
                        displayName: peerDisplayLabel,
                        font: .callout.weight(.semibold),
                        color: .primary)
                    Spacer()
                    if let latest {
                        Text(latest.createdAt.relativeTimeFromUnixSeconds)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                Text(latest?.content ?? "")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 14)
        .contentShape(Rectangle())
        .overlay(alignment: .bottom) {
            Divider().padding(.leading, 48)
        }
        .accessibilityIdentifier("dm-conversation-\(conversation.peerPubkey)")
    }
}

// ── New-message compose sheet ─────────────────────────────────────────────
//
// Starts a NIP-17 conversation with a recipient identified by npub or pubkey.
// Thin-shell: the sheet collects a recipient + body and hands them to
// `DmInboxStore.sendDm`. The kind:14 rumor, gift-wrap, and signing are all
// Rust-owned; recipient-pubkey validation also happens in the actor (which
// surfaces a toast on a malformed key, D6).
//
// Contact picker: backed by `KernelModel.followList` (the active account's
// NIP-02 follow list). Entries carry raw pubkeys; the picker filters the raw
// pubkey as the user types. The manual text field remains as a fallback for
// pasting any pubkey not in the follow list.

private struct DmComposeSheet: View {
    @ObservedObject var store: DmInboxStore
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var recipient = ""
    @State private var draft = ""
    @State private var searchQuery = ""

    private var trimmedRecipient: String {
        recipient.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    private var trimmedDraft: String {
        draft.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    private var canSend: Bool {
        !trimmedRecipient.isEmpty && !trimmedDraft.isEmpty
    }

    /// Follows filtered by `searchQuery` against the raw pubkey or the
    /// Rust-owned resolved profile label. An empty query shows all follows.
    private var filteredFollows: [FollowEntry] {
        let q = searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return model.followList.follows }
        return model.followList.follows.filter {
            DmPeerPresentation.matchesContact(
                pubkey: $0.pubkey,
                profileDisplay: model.profile(forPubkey: $0.pubkey)?.display,
                query: q)
        }
    }

    var body: some View {
        NavigationStack {
            Form {
                // ── Contact picker ──────────────────────────────────────
                if !model.followList.follows.isEmpty {
                    Section {
                        TextField("Search contacts", text: $searchQuery)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .accessibilityIdentifier("dm-compose-contact-search")

                        ForEach(filteredFollows) { follow in
                            Button {
                                recipient = follow.pubkey
                            } label: {
                                let contactLabel = DmPeerPresentation.label(
                                    pubkey: follow.pubkey,
                                    profileDisplay: model.profile(forPubkey: follow.pubkey)?.display)
                                HStack(spacing: 8) {
                                    NostrAvatar(
                                        pubkey: follow.pubkey,
                                        url: nil,
                                        size: 32
                                    )
                                    .equatable()
                                    NostrProfileName(
                                        displayName: contactLabel,
                                        font: .subheadline,
                                        color: .primary)
                                    Spacer()
                                    if recipient == follow.pubkey {
                                        Image(systemName: "checkmark")
                                            .foregroundStyle(ChirpColor.accent)
                                    }
                                }
                            }
                            .accessibilityIdentifier("dm-compose-contact-\(follow.pubkey)")
                        }
                    } header: {
                        Text("Contacts")
                    }
                }

                // ── Manual recipient entry ──────────────────────────────
                Section {
                    TextField("npub1... or hex pubkey", text: $recipient)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("dm-compose-recipient-field")
                } header: {
                    Text("Recipient (npub or pubkey)")
                }

                Section {
                    TextEditor(text: $draft)
                        .frame(minHeight: 100)
                        .accessibilityIdentifier("dm-compose-body-editor")
                } header: {
                    Text("Message")
                }

                Section {
                    Button {
                        send()
                    } label: {
                        Label("Send message", systemImage: "paperplane.fill")
                    }
                    .disabled(!canSend)
                }
            }
            .scrollContentBackground(.hidden)
            .chirpScreenBackground()
            .navigationTitle("New Message")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
    }

    private func send() {
        guard canSend else { return }
        // Fire-and-forget — the sent message surfaces through the next
        // snapshot tick (the actor gift-wraps a self-copy to the sender).
        store.sendDm(to: trimmedRecipient, content: trimmedDraft)
        dismiss()
    }
}
