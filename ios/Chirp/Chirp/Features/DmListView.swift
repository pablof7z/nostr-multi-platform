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
// newest-thread-first from the Rust `DmInboxProjection`; this view only
// renders the list and navigates into a thread. All display strings
// (`peerShortNpub`, `peerAvatarInitials`, `peerAvatarColor`) are computed
// in Rust and consumed verbatim — no bech32 encoding, no pubkey truncation
// in Swift.
// ─────────────────────────────────────────────────────────────────────────

struct DmListView: View {
    @ObservedObject var store: DmInboxStore
    @EnvironmentObject private var model: KernelModel

    @State private var showCompose = false

    var body: some View {
        Group {
            if store.remoteSignerUnsupported {
                bunkerUnsupportedState
            } else if store.conversations.isEmpty {
                emptyState
            } else {
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
                .disabled(store.remoteSignerUnsupported)
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

    private var bunkerUnsupportedState: some View {
        ScrollView {
            ChirpPlaceholder(
                systemImage: "exclamationmark.lock.fill",
                title: "DMs unavailable",
                subtitle: "End-to-end encrypted DMs require a local key.\nBunker (NIP-46) accounts cannot decrypt messages yet."
            )
            .frame(minHeight: 360)
        }
    }

    private var conversationList: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
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
    let conversation: DmConversation

    /// The most recent message — the last entry, since the projection
    /// orders each thread chronologically (oldest first, newest last).
    private var latest: DmMessage? { conversation.messages.last }

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            // Rust pre-computes initials and colour — render verbatim
            // (thin-shell rule: no pubkey truncation or colour derivation here).
            ChirpAvatar(
                url: nil,
                initials: conversation.peerAvatarInitials,
                colorHex: conversation.peerAvatarColor,
                size: 40
            )

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(conversation.peerShortNpub)
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    Spacer()
                    if let latest {
                        Text(latest.createdAtDisplay)
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
// NIP-02 follow list). Each entry is pre-formatted by Rust. The picker
// filters by `shortNpub` as the user types; tapping an entry sets the
// recipient field. The manual text field remains as a fallback for pasting
// any pubkey not in the follow list.

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

    /// Follows filtered by `searchQuery` against the short npub.
    /// An empty query shows all follows (up to the list length).
    private var filteredFollows: [FollowEntry] {
        let q = searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return model.followList.follows }
        return model.followList.follows.filter {
            $0.shortNpub.lowercased().contains(q)
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
                                HStack(spacing: 8) {
                                    ChirpAvatar(
                                        url: nil,
                                        initials: follow.avatarInitials,
                                        colorHex: follow.avatarColor,
                                        size: 32
                                    )
                                    Text(follow.shortNpub)
                                        .font(.subheadline)
                                        .foregroundStyle(.primary)
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

