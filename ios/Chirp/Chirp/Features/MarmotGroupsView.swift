import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// MarmotGroupsView — top-level "Groups" tab root.
//
// Lists the user's MLS encrypted groups (name · member count · unread
// badge) → taps push `MarmotGroupChatView`. A "Pending Invites" section
// surfaces inbound welcomes with Accept / Decline. Toolbar "+" opens a
// create-group sheet (name / description / invitee npubs).
//
// A "NIP-29 Groups" section carries the (unencrypted, relay-managed)
// NIP-29 demo group — the first real consumer of the NIP-29 seam. Tapping
// it pushes `GroupChatView`. This section is ALWAYS present so the NIP-29
// screen is reachable regardless of Marmot (MLS) state.
//
// D6: any nil / decode failure surfaces as the empty state, never a crash —
// the store already collapses every failure to `.empty`.
//
// Key-package status deliberately lives in Settings (SettingsHubView), not
// here, per the milestone scope.
// ─────────────────────────────────────────────────────────────────────────

struct MarmotGroupsView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var showCreate = false

    private var store: MarmotStore { model.marmot }

    var body: some View {
        // The list always renders — the NIP-29 demo section must stay
        // reachable even when the user has no MLS groups / invites.
        groupList
            .chirpScreenBackground()
            .navigationTitle("Groups")
            .navigationBarTitleDisplayMode(.large)
            .toolbar {
                createButton
            }
            .sheet(isPresented: $showCreate) {
                MarmotCreateGroupSheet()
                    .environmentObject(model)
            }
    }

    // ── Group + pending-invite list ───────────────────────────────────────

    private var groupList: some View {
        List {
            nip29Section

            dmSection

            if !store.pendingWelcomes.isEmpty {
                Section {
                    ForEach(store.pendingWelcomes) { welcome in
                        PendingInviteRow(welcome: welcome)
                            .environmentObject(model)
                    }
                } header: {
                    Text("Pending Invites")
                }
            }

            Section {
                if store.groups.isEmpty {
                    Text("No encrypted groups yet")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .padding(.vertical, 4)
                } else {
                    ForEach(store.groups) { group in
                        NavigationLink {
                            MarmotGroupChatView(group: group)
                                .environmentObject(model)
                        } label: {
                            GroupRow(group: group)
                        }
                        .accessibilityIdentifier("marmot-group-row-\(group.idHex)")
                        .accessibilityValue(group.name)
                    }
                }
            } header: {
                Text("Groups")
            }
        }
        .scrollContentBackground(.hidden)
    }

    // ── NIP-29 demo group ─────────────────────────────────────────────────
    //
    // First real consumer of the NIP-29 seam: a `NavigationLink` to
    // `GroupChatView`, backed by `model.groupChat` (a `GroupChatStore`
    // registered via `nmp_app_chirp_register_group_chat`). One fixed demo
    // room — a multi-group app would thread a chosen `GroupId` here.

    private var nip29Section: some View {
        Section {
            NavigationLink {
                GroupChatView(store: model.groupChat)
            } label: {
                NIP29GroupRow(groupId: model.groupChat.groupId)
            }
            .accessibilityIdentifier("nip29-group-row")
            .accessibilityValue(model.groupChat.groupId.localId)
        } header: {
            Text("NIP-29 Groups")
        } footer: {
            Text("Relay-managed group chat (NIP-29). Unencrypted — distinct from the MLS-encrypted groups below.")
        }
    }

    // ── NIP-17 direct messages ────────────────────────────────────────────
    //
    // First consumer of the NIP-17 receive seam: a `NavigationLink` to
    // `DmListView`, backed by `model.dmInbox` (a `DmInboxStore` registered
    // via `nmp_app_chirp_register_dm_inbox`). The inbox is global — every
    // private conversation the local account participates in.

    private var dmSection: some View {
        Section {
            NavigationLink {
                DmListView(store: model.dmInbox)
            } label: {
                DmInboxRow(conversationCount: model.dmInbox.conversations.count)
            }
            .accessibilityIdentifier("nip17-dm-inbox-row")
        } header: {
            Text("Direct Messages")
        } footer: {
            Text("Private, gift-wrapped direct messages (NIP-17). End-to-end encrypted — relays cannot read them.")
        }
    }

    // ── Toolbar: create ───────────────────────────────────────────────────

    @ToolbarContentBuilder
    private var createButton: some ToolbarContent {
        ToolbarItem(placement: .navigationBarTrailing) {
            Button {
                showCreate = true
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 17, weight: .semibold))
            }
            .buttonStyle(.borderless)
            .disabled(!store.isRegistered)
            .accessibilityLabel("Create group")
        }
    }
}

// ── Group row ─────────────────────────────────────────────────────────────

private struct GroupRow: View {
    let group: MarmotGroup

    var body: some View {
        HStack(spacing: 8) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(.quaternary)
                    .frame(width: 40, height: 40)
                Image(systemName: "lock.shield.fill")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(.tint)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(group.name.isEmpty ? "Untitled group" : group.name)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text("\(group.members.count) member\(group.members.count == 1 ? "" : "s")")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            if group.unread > 0 {
                Text("\(group.unread)")
                    .font(.caption2.weight(.bold))
                    .foregroundStyle(.primary)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 3)
                    .background(.quaternary, in: Capsule())
                    .accessibilityLabel("\(group.unread) unread")
            }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

// ── NIP-29 group row ──────────────────────────────────────────────────────

private struct NIP29GroupRow: View {
    let groupId: GroupId

    var body: some View {
        HStack(spacing: 8) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(.quaternary)
                    .frame(width: 40, height: 40)
                Image(systemName: "bubble.left.and.bubble.right.fill")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(.tint)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(groupId.localId)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text(groupId.hostRelayUrl)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer()
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

// ── NIP-17 direct-messages row ────────────────────────────────────────────

private struct DmInboxRow: View {
    let conversationCount: Int

    var body: some View {
        HStack(spacing: 8) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(.quaternary)
                    .frame(width: 40, height: 40)
                Image(systemName: "lock.fill")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(.tint)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text("Messages")
                    .font(.callout.weight(.medium))
                    .foregroundStyle(.primary)
                Text(
                    conversationCount == 0
                        ? "No conversations yet"
                        : "\(conversationCount) conversation\(conversationCount == 1 ? "" : "s")"
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }

            Spacer()
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

// ── Pending invite row ────────────────────────────────────────────────────

private struct PendingInviteRow: View {
    let welcome: MarmotPendingWelcome
    @EnvironmentObject private var model: KernelModel

    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: "envelope.badge.fill")
                    .foregroundStyle(.tint)
                Text(welcome.groupName.isEmpty ? "Group invite" : welcome.groupName)
                    .font(.headline)
                    .foregroundStyle(.primary)
            }
            Text("From \(shortNpub(welcome.inviterNpub))")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack(spacing: 12) {
                Button {
                    busy = true
                    _ = model.marmot.acceptWelcome(welcomeIDHex: welcome.idHex)
                    busy = false
                } label: {
                    Text("Accept")
                        .font(.callout.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("marmot-accept-invite-\(welcome.idHex)")

                Button {
                    busy = true
                    _ = model.marmot.declineWelcome(welcomeIDHex: welcome.idHex)
                    busy = false
                } label: {
                    Text("Decline")
                        .font(.callout.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                }
                .buttonStyle(.bordered)
            }
            .disabled(busy)
            .opacity(busy ? 0.5 : 1.0)
        }
        .padding(.vertical, 4)
    }

    private func shortNpub(_ npub: String) -> String {
        guard npub.count >= 16 else { return npub }
        return "\(npub.prefix(10))…\(npub.suffix(6))"
    }
}

// ── Create-group sheet ────────────────────────────────────────────────────

struct MarmotCreateGroupSheet: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var groupDescription = ""
    @State private var inviteeText = ""
    @State private var errorMessage: String?
    @State private var busy = false

    private var trimmedName: String {
        name.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var invitees: [String] {
        inviteeText
            .split(whereSeparator: { $0 == "," || $0 == "\n" || $0 == " " })
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    field("Group name", text: $name, placeholder: "Trusted circle")
                    field("Description", text: $groupDescription, placeholder: "Optional")
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Invitee npubs")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        TextEditor(text: $inviteeText)
                            .font(.body.monospaced())
                            .frame(minHeight: 90)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .overlay(alignment: .topLeading) {
                                if inviteeText.isEmpty {
                                    Text("npub1…, npub1… (comma or newline separated)")
                                        .font(.body.monospaced())
                                        .foregroundStyle(.secondary)
                                        .allowsHitTesting(false)
                                        .padding(.top, 8)
                                }
                            }
                    }
                }

                if let errorMessage {
                    Section {
                        Text(errorMessage)
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                }

                Section {
                    Button {
                        create()
                    } label: {
                        Label("Create group", systemImage: "lock.shield.fill")
                    }
                    .disabled(trimmedName.isEmpty || busy)
                }
            }
            .scrollContentBackground(.hidden)
            .chirpScreenBackground()
            .navigationTitle("New Group")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
    }

    private func create() {
        busy = true
        errorMessage = nil
        let result = model.marmot.createGroup(
            name: trimmedName,
            description: groupDescription.trimmingCharacters(in: .whitespacesAndNewlines),
            inviteeNpubs: invitees)
        busy = false
        if result.ok {
            dismiss()
        } else if let needs = result.needs, !needs.isEmpty {
            errorMessage = "Fetching key packages for \(needs.map(shortNpub).joined(separator: ", "))… tap Create again in a moment."
        } else {
            errorMessage = result.error ?? "Could not create group"
        }
    }

    @ViewBuilder
    private func field(_ label: String, text: Binding<String>, placeholder: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
            TextField(placeholder, text: text)
                .font(.body)
                .foregroundStyle(.primary)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
        }
    }

    private func shortNpub(_ npub: String) -> String {
        guard npub.count >= 16 else { return npub }
        return "\(npub.prefix(8))…\(npub.suffix(4))"
    }
}
