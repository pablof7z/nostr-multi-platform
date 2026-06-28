import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// GroupChatView — one NIP-29 group's chat stream.
//
// First real consumer of the NIP-29 seam:
//   • Read:  `projections["nmp.nip29.group_events"]`, mirrored by `GroupChatStore`
//            (registered via `nmp_app_chirp_register_group_events`).
//   • Write: `nmp.nip29.publish_group_event` via `KernelHandle.postChatMessage`.
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
    /// The message currently being replied to, or `nil` for a plain post.
    /// Set by a context-menu "Reply" tap; cleared on send or banner dismiss.
    @State private var replyTarget: GroupEvent?
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
                        GroupChatMessageRow(
                            message: message,
                            onReact: {
                                store.reactToMessage(
                                    eventId: message.id,
                                    eventAuthorPubkey: message.pubkey)
                            },
                            onReply: {
                                replyTarget = message
                                composerFocused = true
                            })
                    }
                }
                .padding(.vertical, 4)
            }
        }
    }

    // ── Composer ──────────────────────────────────────────────────────────

    private var composer: some View {
        VStack(spacing: 0) {
            replyBanner
            composerInput
        }
        .background(ChirpColor.bg)
        .overlay(alignment: .top) { Divider() }
    }

    /// "Replying to…" banner shown above the composer when a reply target is
    /// set. Tapping the dismiss chip clears the target — the next send reverts
    /// to a plain kind:9 chat message.
    @ViewBuilder
    private var replyBanner: some View {
        if let replyTarget {
            HStack(spacing: 8) {
                Image(systemName: "arrowshape.turn.up.left.fill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Replying to \(replyTarget.pubkey.shortHex)")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.primary)
                    Text(replyTarget.content)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer()
                Button {
                    self.replyTarget = nil
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 18))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Cancel reply")
                .accessibilityIdentifier("group-chat-cancel-reply-button")
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(.quaternary)
            .accessibilityIdentifier("group-chat-reply-banner")
            .overlay(alignment: .bottom) { Divider() }
        }
    }

    private var composerInput: some View {
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
                        trimmedDraft.isEmpty ? ChirpColor.textSecondary : ChirpColor.accent)
            }
            .buttonStyle(.plain)
            .disabled(trimmedDraft.isEmpty)
            .accessibilityLabel("Send")
            .accessibilityIdentifier("group-chat-send-button")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    private func sendDraft() {
        let text = trimmedDraft
        guard !text.isEmpty else { return }
        // Fire-and-forget: the sent message reappears via the next snapshot
        // tick. Clearing the draft optimistically matches `ComposeView` /
        // `MarmotGroupChatView`. A non-nil `replyTarget` routes the send to
        // `nmp.nip29.comment_in_group` (a kind:1111 reply); the verb choice is
        // the store's, not the view's (thin-shell rule).
        store.sendMessage(text, replyToEventId: replyTarget?.id)
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        draft = ""
        replyTarget = nil
    }
}

// ── Message row ───────────────────────────────────────────────────────────

private struct GroupChatMessageRow: View {
    let message: GroupEvent
    /// Long-press → "React ❤️": dispatches `nmp.nip29.react_in_group`.
    let onReact: () -> Void
    /// Long-press → "Reply": arms the composer's reply target so the next
    /// send dispatches `nmp.nip29.comment_in_group`.
    let onReply: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            NostrAvatar(
                pubkey: message.pubkey,
                url: nil,
                size: 36
            )
            .equatable()

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(message.pubkey.shortHex)
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    Text(message.createdAt.relativeTimeFromUnixSeconds)
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
        .contentShape(Rectangle())
        .overlay(alignment: .bottom) {
            Divider().padding(.leading, 44)
        }
        .accessibilityIdentifier("group-chat-message-\(message.id)")
        // Long-press context menu — the only entry point for the two NIP-29
        // composed actions. The menu items only marshal intent; the kind:7 /
        // kind:1111 event shapes are Rust-owned (thin-shell rule).
        .contextMenu {
            Button {
                onReact()
            } label: {
                Label("React ❤️", systemImage: "heart")
            }
            .accessibilityIdentifier("group-chat-react-button")

            Button {
                onReply()
            } label: {
                Label("Reply", systemImage: "arrowshape.turn.up.left")
            }
            .accessibilityIdentifier("group-chat-reply-button")
        }
    }
}
