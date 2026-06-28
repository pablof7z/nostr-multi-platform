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
    @State private var showMembers = false
    @State private var sending = false
    /// Correlation id of an in-flight `leave` op. The chat view dismisses only
    /// when this op reaches a terminal `.accepted` verdict — leaving on mere
    /// submission would bounce the user out of a group they may still be in if
    /// the op fails (same dismiss-only-on-terminal rule as NewGroupSheet).
    @State private var leaveCid: String?
    /// Rust-owned terminal failure reason for a failed leave, rendered verbatim.
    @State private var leaveError: String?
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
        .sheet(isPresented: $showMembers) {
            MarmotMembersSheet(
                // ADR-0032: shell-side abbreviation of the raw hex pubkeys.
                members: liveGroup.members.map { $0.shortHex },
                onDone: { showMembers = false }
            )
        }
        .task(id: model.rev) { reloadMessages() }
        .onAppear { reloadMessages() }
        .onChange(of: model.actionLifecycle) { resolveLeaveTerminal() }
        .alert(
            "Could not leave group",
            isPresented: Binding(get: { leaveError != nil }, set: { if !$0 { leaveError = nil } })
        ) {
            Button("OK", role: .cancel) { leaveError = nil }
        } message: {
            Text(leaveError ?? "")
        }
    }

    /// Matches `leaveCid` against `recentTerminal` — same seam as NewGroupSheet
    /// / RelaySettingsView. Dismisses only on `.accepted`; surfaces the
    /// Rust-owned reason verbatim on `.failed`.
    private func resolveLeaveTerminal() {
        guard let cid = leaveCid,
              let entry = model.recentTerminal(correlationId: cid) else { return }
        leaveCid = nil
        switch entry.stage {
        case .accepted:
            dismiss()
        case .failed:
            // #1735: prefer the localized reason_code, falling back to prose.
            let reason = entry.stage.localizedReason ?? ""
            leaveError = reason.isEmpty ? "Leaving the group failed." : reason
        default:
            break
        }
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
                            ? ChirpColor.textSecondary
                            : ChirpColor.accent)
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
        Task {
            let result = await model.marmot.send(groupIDHex: group.idHex, text: text)
            sending = false
            if result.ok {
                UIImpactFeedbackGenerator(style: .light).impactOccurred()
                draft = ""
                reloadMessages()
            }
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
                Button {
                    showMembers = true
                } label: {
                    // ADR-0032: pluralisation lives in the presentation layer.
                    Text("\(liveGroup.memberCount) \(liveGroup.memberCount == 1 ? "member" : "members")")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Show members")
                .accessibilityIdentifier("marmot-show-members")
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
                    Task {
                        let result = await model.marmot.leave(groupIDHex: group.idHex)
                        // Dismiss only on terminal `.accepted` (resolveLeaveTerminal);
                        // stash the cid and wait rather than leaving on submission.
                        if result.ok, let cid = result.correlationId {
                            leaveCid = cid
                        } else if let err = result.error {
                            leaveError = err
                        }
                    }
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
// ADR-0032: `MarmotMessage` carries the raw sender pubkey (hex) and the
// raw `created_at` timestamp. This row derives the abbreviated pubkey
// label and relative-time stamp locally via `PubkeyFormatting.swift`.

private struct MarmotMessageRow: View {
    let message: MarmotMessage

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            NostrAvatar(
                pubkey: message.senderPubkeyHex,
                url: nil,
                size: 36
            )
            .equatable()

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(message.senderPubkeyHex.shortHex)
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
        .overlay(alignment: .bottom) {
            Divider()
                .padding(.leading, 44)
        }
        .accessibilityIdentifier("marmot-message-\(message.id)")
    }
}

// ── Members sheet (V-17) ──────────────────────────────────────────────────
//
// Thin-shell render of `liveGroup.membersDisplay`. The Rust projection has
// already converted each hex pubkey to an abbreviated bech32 npub — Swift
// only renders the strings (no hex→npub conversion, no business logic).

private struct MarmotMembersSheet: View {
    let members: [String]
    let onDone: () -> Void

    var body: some View {
        NavigationView {
            List(members, id: \.self) { npub in
                Text(npub)
                    .font(.body.monospaced())
                    .accessibilityIdentifier("marmot-member-\(npub)")
            }
            .navigationTitle("Members")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button("Done", action: onDone)
                        .accessibilityIdentifier("marmot-members-done")
                }
            }
        }
    }
}
