import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// MarmotGroupsView — top-level "Groups" tab root.
//
// Lists the user's MLS encrypted groups (name · member count · unread
// badge) → taps push `MarmotGroupChatView`. A "Pending Invites" section
// surfaces inbound welcomes with Accept / Decline. Toolbar "+" opens a
// create-group sheet (name / description / invitee npubs).
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
    @State private var polling = false

    private var store: MarmotStore { model.marmot }

    var body: some View {
        ZStack {
            if isEmpty {
                emptyState
            } else {
                groupList
            }
        }
        .chirpScreenBackground()
        .navigationTitle("Groups")
        .navigationBarTitleDisplayMode(.large)
        .toolbar {
            ToolbarItem(placement: .navigationBarLeading) {
                Button {
                    guard !polling else { return }
                    polling = true
                    Task.detached(priority: .userInitiated) {
                        _ = await MainActor.run {
                            model.marmot.pollInbox(extraRelays: ["wss://nos.lol", "wss://relay.primal.net"])
                        }
                        await MainActor.run { polling = false }
                    }
                } label: {
                    if polling {
                        ProgressView().controlSize(.small)
                    } else {
                        Image(systemName: "arrow.clockwise")
                    }
                }
                .disabled(!store.isRegistered || polling)
                .accessibilityLabel("Poll MLS inbox")
            }
            createButton
        }
        .sheet(isPresented: $showCreate) {
            MarmotCreateGroupSheet()
                .environmentObject(model)
        }
    }

    private var isEmpty: Bool {
        store.groups.isEmpty && store.pendingWelcomes.isEmpty
    }

    // ── Group + pending-invite list ───────────────────────────────────────

    private var groupList: some View {
        List {
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
                    }
                }
            } header: {
                Text("Groups")
            }
        }
        .animation(.smooth, value: store.groups.count)
        .animation(.smooth, value: store.pendingWelcomes.count)
        .scrollContentBackground(.hidden)
    }

    // ── Empty / not-registered state ──────────────────────────────────────

    private var emptyState: some View {
        ScrollView {
            VStack(spacing: 24) {
                ChirpPlaceholder(
                    systemImage: "lock.shield.fill",
                    title: "Encrypted Groups",
                    subtitle: store.isRegistered
                        ? "No groups yet. Tap + to create an MLS-encrypted group."
                        : "Sign in with an nsec to enable Marmot encrypted groups."
                )
                if store.isRegistered {
                    Button {
                        guard !polling else { return }
                        polling = true
                        Task.detached(priority: .userInitiated) {
                            _ = await MainActor.run {
                                model.marmot.pollInbox(extraRelays: ["wss://nos.lol", "wss://relay.primal.net"])
                            }
                            await MainActor.run { polling = false }
                        }
                    } label: {
                        Label(polling ? "Polling…" : "Poll Inbox", systemImage: "arrow.clockwise")
                            .font(.headline)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 14)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(polling)
                    .padding(.horizontal, 32)
                }
            }
            .frame(minHeight: 500)
            .padding(.horizontal, ChirpSpace.l)
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
                    .chirpGlass(cornerRadius: 12)
                    .accessibilityLabel("\(group.unread) unread")
            }
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
            ZStack {
                ChirpBackdrop()
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        VStack(alignment: .leading, spacing: 12) {
                            field("Group name", text: $name, placeholder: "Trusted circle")
                            Divider()
                            field("Description", text: $groupDescription,
                                  placeholder: "Optional")
                            Divider()
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
                        .padding(.horizontal, 16)
                        .padding(.vertical, 12)
                        .chirpGlass(cornerRadius: ChirpSpace.radius)
                        .padding(.horizontal, 16)

                        if let errorMessage {
                            Text(errorMessage)
                                .font(.caption)
                                .foregroundStyle(.red)
                                .padding(.horizontal, 16)
                        }

                        Button {
                            create()
                        } label: {
                            HStack {
                                Image(systemName: "lock.shield.fill")
                                Text("Create group")
                            }
                            .font(.headline)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 12)
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(trimmedName.isEmpty || busy)
                        .opacity(trimmedName.isEmpty || busy ? 0.45 : 1.0)
                        .padding(.horizontal, 16)
                        .padding(.bottom, 32)
                    }
                    .padding(.top, 16)
                }
            }
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
