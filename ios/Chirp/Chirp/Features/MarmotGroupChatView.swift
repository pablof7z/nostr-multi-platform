import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// MarmotGroupChatView — one MLS encrypted group's message stream.
//
// • Message rows reuse the NoteRow visual idiom (abbreviated sender npub +
//   content + relative time). Every display string — `senderShort`,
//   `senderInitials`, `senderColorHex`, `createdAtDisplay` — is supplied
//   by Rust in `MarmotMessage`; Swift renders verbatim.
// • Composer reuses the ComposeView idiom.
// • Header shows member count + an Invite button (→ MarmotInviteSheet).
// • Overflow menu carries the "Leave group" destructive action.
//
// Thin-shell rule (chirp/AGENTS.md "canonical bad example"): no
// `.filter` / `.sorted` / `.reduce` / `RelativeDateTimeFormatter` /
// `JSONDecoder` / `switch` on protocol semantics. The "live group" lookup
// is a typed dictionary index built once per snapshot in `MarmotStore`.
// ─────────────────────────────────────────────────────────────────────────

struct MarmotGroupChatView: View {
    let group: MarmotGroup

    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var messages: [MarmotMessage] = []
    @State private var draft = ""
    @State private var showInvite = false
    @State private var sending = false
    @FocusState private var composerFocused: Bool

    /// Live group row from the snapshot lookup; falls back to the
    /// constructor-passed value when the row has disappeared. The lookup
    /// itself lives in `MarmotStore` (render infrastructure, not view
    /// logic).
    private var liveGroup: MarmotGroup {
        model.marmot.group(idHex: group.idHex, fallback: group)
    }

    private var trimmedDraft: String {
        draft.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        VStack(spacing: 0) {
            messageStream
            composer
        }
        .chirpScreenBackground()
        .navigationTitle(liveGroup.displayName)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { toolbarContent }
        .sheet(isPresented: $showInvite) {
            MarmotInviteSheet(group: liveGroup)
                .environmentObject(model)
        }
        .task(id: model.rev) { reloadMessages() }
        .onAppear { reloadMessages() }
    }

    private func reloadMessages() {
        messages = model.marmot.messages(groupIDHex: group.idHex)
    }

    // ── Message stream ────────────────────────────────────────────────────

    @ViewBuilder
    private var messageStream: some View {
        if messages.isEmpty {
            ScrollView {
                ChirpPlaceholder(
                    systemImage: "lock.fill",
                    title: "No messages",
                    subtitle: "Messages in this group are end-to-end encrypted with MLS."
                )
                .frame(minHeight: 360)
            }
        } else {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(messages) { message in
                            MarmotMessageRow(message: message)
                                .id(message.id)
                        }
                    }
                    .padding(.vertical, 4)
                    .padding(.horizontal, 12)
                }
                .onChange(of: messages.count) { _, _ in
                    if let last = messages.last {
                        proxy.scrollTo(last.id, anchor: .bottom)
                    }
                }
                .onAppear {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                        if let last = messages.last {
                            proxy.scrollTo(last.id, anchor: .bottom)
                        }
                    }
                }
            }
        }
    }

    // ── Composer (ComposeView idiom) ──────────────────────────────────────

    private var composer: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextEditor(text: $draft)
                .focused($composerFocused)
                .font(.body)
                .foregroundStyle(.primary)
                .frame(minHeight: 38, maxHeight: 120)
                .accessibilityIdentifier("marmot-message-editor")
                .overlay(alignment: .topLeading) {
                    if draft.isEmpty {
                        Text("Encrypted message…")
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
                        trimmedDraft.isEmpty || sending
                            ? Color.secondary
                            : Color.accentColor)
            }
            .buttonStyle(.plain)
            .disabled(trimmedDraft.isEmpty || sending)
            .accessibilityLabel("Send")
            .accessibilityIdentifier("marmot-send-button")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(ChirpColor.bg)
        .overlay(alignment: .top) {
            Divider()
        }
    }

    private func sendDraft() {
        let text = trimmedDraft
        guard !text.isEmpty else { return }
        sending = true
        let result = model.marmot.send(groupIDHex: group.idHex, text: text)
        sending = false
        if result.ok {
            UIImpactFeedbackGenerator(style: .light).impactOccurred()
            draft = ""
            reloadMessages()
        }
    }

    // ── Toolbar: invite + leave ───────────────────────────────────────────

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItem(placement: .principal) {
            VStack(spacing: 1) {
                Text(liveGroup.displayName)
                    .font(.headline)
                    .foregroundStyle(.primary)
                Text(liveGroup.memberCountDisplay)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        ToolbarItem(placement: .navigationBarTrailing) {
            Menu {
                Button {
                    showInvite = true
                } label: {
                    Label("Invite members", systemImage: "person.badge.plus")
                }
                Button(role: .destructive) {
                    let result = model.marmot.leave(groupIDHex: group.idHex)
                    if result.ok { dismiss() }
                } label: {
                    Label("Leave group", systemImage: "rectangle.portrait.and.arrow.right")
                }
            } label: {
                Image(systemName: "ellipsis.circle")
                    .font(.system(size: 17, weight: .semibold))
            }
            .accessibilityLabel("Group options")
        }
    }
}

// ── Message row (NoteRow idiom) ───────────────────────────────────────────
//
// Every label here is rendered verbatim from `MarmotMessage`. The Rust
// projection pre-computes the abbreviated npub, the 2-char initials, the
// 6-hex avatar tint, and the relative-time stamp.

private struct MarmotMessageRow: View {
    let message: MarmotMessage

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            ChirpAvatar(
                url: nil,
                initials: message.senderInitials,
                colorHex: message.senderColorHex,
                size: 36
            )

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(message.senderShort)
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    Text(message.createdAtDisplay)
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
            Divider()
                .padding(.leading, 44)
        }
        .accessibilityIdentifier("marmot-message-\(message.id)")
    }
}
