import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// GroupChatView — one NIP-29 group's chat stream.
//
// First real consumer of the NIP-29 seam:
//   • Read:  `projections["nip29.group_chat"]`, mirrored by `GroupChatStore`
//            (registered via `nmp_app_chirp_register_group_chat`).
//   • Write: `nip29.post_chat_message` via `KernelHandle.postChatMessage`.
//
// Thin-shell rule: ZERO protocol logic here. Messages arrive newest-first
// from the Rust `GroupChatProjection`; this view only renders them and
// hands raw draft text to `GroupChatStore.sendMessage`.
//
// Functional-first styling — message rows reuse the abbreviated-hex +
// content + relative-time idiom from `MarmotGroupChatView`; the composer
// reuses its inline-TextEditor idiom.
// ─────────────────────────────────────────────────────────────────────────

struct GroupChatView: View {
    @ObservedObject var store: GroupChatStore

    @State private var draft = ""
    @FocusState private var composerFocused: Bool

    private var trimmedDraft: String {
        draft.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        VStack(spacing: 0) {
            messageStream
            composer
        }
        .chirpScreenBackground()
        .navigationTitle(store.groupId.localId)
        .navigationBarTitleDisplayMode(.inline)
    }

    // ── Message stream (newest-first, per the projection output) ──────────

    @ViewBuilder
    private var messageStream: some View {
        if store.messages.isEmpty {
            ScrollView {
                ChirpPlaceholder(
                    systemImage: "bubble.left.and.bubble.right",
                    title: "No messages",
                    subtitle: "Messages posted to this NIP-29 group appear here."
                )
                .frame(minHeight: 360)
            }
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    // The projection emits newest-first; render in that
                    // order — no Swift-side re-sort (thin-shell rule).
                    ForEach(store.messages) { message in
                        GroupChatMessageRow(message: message)
                    }
                }
                .padding(.vertical, 4)
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
                .accessibilityIdentifier("group-chat-message-editor")
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
            .accessibilityIdentifier("group-chat-send-button")
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
        // tick. Clearing the draft optimistically matches `ComposeView` /
        // `MarmotGroupChatView`.
        store.sendMessage(text)
        draft = ""
    }
}

// ── Message row ───────────────────────────────────────────────────────────

private struct GroupChatMessageRow: View {
    let message: GroupChatMessage

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            ZStack {
                Circle().fill(.quaternary)
                Text(initials)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.primary)
            }
            .frame(width: 36, height: 36)
            .overlay(Circle().stroke(Color(.separator), lineWidth: 1))

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(shortPubkey(message.pubkey))
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    Text(relativeTime(message.createdAt))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                Text(message.content)
                    .font(.body)
                    .foregroundStyle(.primary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 14)
        .overlay(alignment: .bottom) {
            Divider().padding(.leading, 44)
        }
        .accessibilityIdentifier("group-chat-message-\(message.id)")
    }

    /// First two hex chars of the author pubkey — a cheap deterministic
    /// avatar label. No npub decoding (thin-shell: no protocol logic).
    private var initials: String {
        String(message.pubkey.prefix(2)).uppercased()
    }

    /// Truncated hex pubkey: `abcdef01…23456789`.
    private func shortPubkey(_ hex: String) -> String {
        guard hex.count >= 16 else { return hex }
        return "\(hex.prefix(8))…\(hex.suffix(8))"
    }

    private func relativeTime(_ unixSecs: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(unixSecs))
        let fmt = RelativeDateTimeFormatter()
        fmt.unitsStyle = .abbreviated
        return fmt.localizedString(for: date, relativeTo: Date())
    }
}
