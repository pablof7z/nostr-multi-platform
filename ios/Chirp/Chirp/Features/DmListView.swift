import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// DmListView — the NIP-17 private direct-message inbox.
//
// First consumer of the NIP-17 receive seam:
//   • Read:  `projections["nip17.dm_inbox"]`, mirrored by `DmInboxStore`
//            (registered via `nmp_app_chirp_register_dm_inbox`).
//   • Write: `nmp.dm.send` via `KernelHandle.sendDm` — reached from
//            `DmConversationView`.
//
// Thin-shell rule: ZERO protocol logic here. Conversations arrive
// newest-thread-first from the Rust `DmInboxProjection`; this view only
// renders the list and navigates into a thread.
// ─────────────────────────────────────────────────────────────────────────

struct DmListView: View {
    @ObservedObject var store: DmInboxStore

    @State private var showCompose = false

    var body: some View {
        Group {
            if store.conversations.isEmpty {
                emptyState
            } else {
                conversationList
            }
        }
        .chirpScreenBackground()
        .navigationTitle("Messages")
        .navigationBarTitleDisplayMode(.inline)
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
            }
        }
        .sheet(isPresented: $showCompose) {
            DmComposeSheet(store: store)
        }
    }

    private var emptyState: some View {
        ScrollView {
            ChirpPlaceholder(
                systemImage: "lock.fill",
                title: "No messages",
                subtitle: "Private NIP-17 direct messages you send and receive appear here."
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

    /// The most recent message — index 0, since the projection orders each
    /// thread newest-first.
    private var latest: DmMessage? { conversation.messages.first }

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            ZStack {
                Circle().fill(.quaternary)
                Text(dmInitials(conversation.peerPubkey))
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.primary)
            }
            .frame(width: 40, height: 40)
            .overlay(Circle().stroke(Color(.separator), lineWidth: 1))

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(dmShortPubkey(conversation.peerPubkey))
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    Spacer()
                    if let latest {
                        Text(dmRelativeTime(latest.createdAt))
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
// Starts a NIP-17 conversation with a recipient identified by hex pubkey.
// Thin-shell: the sheet only collects a recipient + body and hands them to
// `DmInboxStore.sendDm`. The kind:14 rumor, gift-wrap, and signing are all
// Rust-owned; recipient-pubkey validation also happens in the actor (which
// surfaces a toast on a malformed key, D6).

private struct DmComposeSheet: View {
    @ObservedObject var store: DmInboxStore
    @Environment(\.dismiss) private var dismiss

    @State private var recipient = ""
    @State private var draft = ""

    private var trimmedRecipient: String {
        recipient.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    private var trimmedDraft: String {
        draft.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    private var canSend: Bool {
        !trimmedRecipient.isEmpty && !trimmedDraft.isEmpty
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Recipient pubkey (hex)", text: $recipient)
                        .font(.body.monospaced())
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("dm-compose-recipient-field")
                } header: {
                    Text("To")
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

// ── Shared helpers (thin-shell: pure display, no protocol decoding) ───────

/// Truncated hex pubkey: `abcdef01…23456789`. No npub decoding — that would
/// be protocol logic, which belongs in Rust.
func dmShortPubkey(_ hex: String) -> String {
    guard hex.count >= 16 else { return hex.isEmpty ? "Unknown" : hex }
    return "\(hex.prefix(8))…\(hex.suffix(8))"
}

/// First two hex chars of a pubkey — a cheap deterministic avatar label.
func dmInitials(_ hex: String) -> String {
    guard !hex.isEmpty else { return "?" }
    return String(hex.prefix(2)).uppercased()
}

/// Abbreviated relative time for a unix-seconds timestamp.
func dmRelativeTime(_ unixSecs: UInt64) -> String {
    let date = Date(timeIntervalSince1970: TimeInterval(unixSecs))
    let fmt = RelativeDateTimeFormatter()
    fmt.unitsStyle = .abbreviated
    return fmt.localizedString(for: date, relativeTo: Date())
}
