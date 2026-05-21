import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// MarmotGroupChatView — one MLS encrypted group's message stream.
//
// • Message rows reuse the NoteRow visual idiom (abbreviated sender npub +
//   content + relative time), styled with the frozen design system.
// • Composer reuses the ComposeView idiom (TextEditor inline at the bottom,
//   calling `send`).
// • Header shows member count + an Invite button (→ MarmotInviteSheet).
// • Overflow menu carries the "Leave group" destructive action.
//
// Messages are pulled on appear and on every kernel tick (the group's
// `last_msg_at` / `unread` in the snapshot changes drive a re-pull). D6:
// `messages(groupIDHex:)` returns `[]` on any failure — never crashes.
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

    /// Live group record from the snapshot (member count / name can change
    /// out from under us via evolution events) — fall back to the value we
    /// were constructed with.
    private var liveGroup: MarmotGroup {
        model.marmot.groups.first(where: { $0.idHex == group.idHex }) ?? group
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
        .navigationTitle(liveGroup.name.isEmpty ? "Group" : liveGroup.name)
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
            draft = ""
            reloadMessages()
        }
    }

    // ── Toolbar: invite + leave ───────────────────────────────────────────

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItem(placement: .principal) {
            VStack(spacing: 1) {
                Text(liveGroup.name.isEmpty ? "Group" : liveGroup.name)
                    .font(.headline)
                    .foregroundStyle(.primary)
                Text("\(liveGroup.members.count) member\(liveGroup.members.count == 1 ? "" : "s")")
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

private struct MarmotMessageRow: View {
    let message: MarmotMessage

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            ChirpAvatar(
                url: nil,
                initials: initials,
                colorHex: colorHex,
                size: 36
            )

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(shortNpub(message.senderNpub))
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
            Divider()
                .padding(.leading, 44)
        }
        .accessibilityIdentifier("marmot-message-\(message.id)")
    }

    private var colorHex: String {
        // Deterministic gradient seed from the npub tail.
        let tail = String(message.senderNpub.suffix(6))
        var hash: UInt32 = 5381
        for b in tail.utf8 { hash = (hash &* 33) &+ UInt32(b) }
        return String(format: "%06X", hash & 0xFFFFFF)
    }

    private var initials: String {
        let trimmed = message.senderNpub.hasPrefix("npub1")
            ? String(message.senderNpub.dropFirst(5))
            : message.senderNpub
        return String(trimmed.prefix(2)).uppercased()
    }

    private func shortNpub(_ npub: String) -> String {
        guard npub.count >= 16 else { return npub }
        return "\(npub.prefix(10))…\(npub.suffix(6))"
    }

    private func relativeTime(_ unixSecs: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(unixSecs))
        let fmt = RelativeDateTimeFormatter()
        fmt.unitsStyle = .abbreviated
        return fmt.localizedString(for: date, relativeTo: Date())
    }
}
