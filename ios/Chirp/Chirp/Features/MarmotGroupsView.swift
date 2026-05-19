import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// MarmotGroupsView — top-level "Groups" tab root.
//
// Lists the user's MLS encrypted groups (name · member count · unread
// badge) → taps push `MarmotGroupChatView`. A "Pending Invites" section
// surfaces inbound welcomes with Accept / Decline. Toolbar "+" opens a
// create-group sheet (name / description / invitee npubs).
//
// Reuses the frozen Chirp design system (ChirpColor / ChirpFont /
// ChirpSpace / GlassCard / ChirpPlaceholder / ChirpPrimaryButton). D6: any
// nil / decode failure surfaces as the empty state, never a crash — the
// store already collapses every failure to `.empty`.
//
// Key-package status deliberately lives in Settings (SettingsHubView), not
// here, per the milestone scope.
// ─────────────────────────────────────────────────────────────────────────

struct MarmotGroupsView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var showCreate = false

    private var store: MarmotStore { model.marmot }

    var body: some View {
        ZStack {
            if isEmpty {
                emptyState
            } else {
                groupList
            }
        }
        .navigationTitle("Groups")
        .navigationBarTitleDisplayMode(.large)
        .toolbar { createButton }
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
                            .listRowBackground(Color.clear)
                            .listRowSeparator(.hidden)
                    }
                } header: {
                    ChirpSectionHeader(title: "Pending Invites")
                }
            }

            Section {
                if store.groups.isEmpty {
                    Text("No encrypted groups yet")
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textTertiary)
                        .padding(.vertical, ChirpSpace.xs)
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                } else {
                    ForEach(store.groups) { group in
                        NavigationLink {
                            MarmotGroupChatView(group: group)
                                .environmentObject(model)
                        } label: {
                            GroupRow(group: group)
                        }
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                    }
                }
            } header: {
                ChirpSectionHeader(title: "Groups")
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(ChirpColor.bg)
        .animation(.smooth, value: store.groups.count)
        .animation(.smooth, value: store.pendingWelcomes.count)
    }

    // ── Empty / not-registered state ──────────────────────────────────────

    private var emptyState: some View {
        ScrollView {
            ChirpPlaceholder(
                systemImage: "lock.shield.fill",
                title: "Encrypted Groups",
                subtitle: store.isRegistered
                    ? "No groups yet. Tap + to create an MLS-encrypted group."
                    : "Sign in with an nsec to enable Marmot encrypted groups."
            )
            .frame(minHeight: 500)
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
                    .foregroundStyle(ChirpColor.accent)
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
        HStack(spacing: ChirpSpace.m) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(ChirpColor.accentSoft)
                    .frame(width: 40, height: 40)
                Image(systemName: "lock.shield.fill")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(ChirpColor.accent)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(group.name.isEmpty ? "Untitled group" : group.name)
                    .font(ChirpFont.callout.weight(.medium))
                    .foregroundStyle(ChirpColor.textPrimary)
                    .lineLimit(1)
                Text("\(group.members.count) member\(group.members.count == 1 ? "" : "s")")
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)
            }

            Spacer()

            if group.unread > 0 {
                Text("\(group.unread)")
                    .font(.system(.caption2, design: .rounded).weight(.bold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 3)
                    .background(ChirpColor.accent, in: Capsule())
                    .accessibilityLabel("\(group.unread) unread")
            }
        }
        .padding(.vertical, ChirpSpace.xs)
        .contentShape(Rectangle())
    }
}

// ── Pending invite row ────────────────────────────────────────────────────

private struct PendingInviteRow: View {
    let welcome: MarmotPendingWelcome
    @EnvironmentObject private var model: KernelModel

    @State private var busy = false

    var body: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: ChirpSpace.s) {
                HStack(spacing: ChirpSpace.s) {
                    Image(systemName: "envelope.badge.fill")
                        .foregroundStyle(ChirpColor.accent)
                    Text(welcome.groupName.isEmpty ? "Group invite" : welcome.groupName)
                        .font(ChirpFont.headline)
                        .foregroundStyle(ChirpColor.textPrimary)
                }
                Text("From \(shortNpub(welcome.inviterNpub))")
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)

                HStack(spacing: ChirpSpace.m) {
                    Button {
                        busy = true
                        _ = model.marmot.acceptWelcome(welcomeIDHex: welcome.idHex)
                        busy = false
                    } label: {
                        Text("Accept")
                            .font(ChirpFont.callout.weight(.semibold))
                            .foregroundStyle(.white)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, ChirpSpace.s)
                            .background(ChirpColor.accent, in: Capsule())
                    }
                    .buttonStyle(.plain)

                    Button {
                        busy = true
                        _ = model.marmot.declineWelcome(welcomeIDHex: welcome.idHex)
                        busy = false
                    } label: {
                        Text("Decline")
                            .font(ChirpFont.callout.weight(.semibold))
                            .foregroundStyle(ChirpColor.textSecondary)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, ChirpSpace.s)
                            .background(ChirpColor.surface, in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
                .disabled(busy)
                .opacity(busy ? 0.5 : 1.0)
            }
        }
        .padding(.vertical, ChirpSpace.xs)
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
                Color(.systemBackground).ignoresSafeArea()
                ScrollView {
                    VStack(alignment: .leading, spacing: ChirpSpace.l) {
                        GlassCard {
                            VStack(alignment: .leading, spacing: ChirpSpace.m) {
                                field("Group name", text: $name, placeholder: "Trusted circle")
                                Divider().background(ChirpColor.hairline)
                                field("Description", text: $groupDescription,
                                      placeholder: "Optional")
                                Divider().background(ChirpColor.hairline)
                                VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                                    Text("Invitee npubs")
                                        .font(ChirpFont.caption)
                                        .foregroundStyle(ChirpColor.textTertiary)
                                    TextEditor(text: $inviteeText)
                                        .font(ChirpFont.mono)
                                        .scrollContentBackground(.hidden)
                                        .frame(minHeight: 90)
                                        .textInputAutocapitalization(.never)
                                        .autocorrectionDisabled()
                                        .overlay(alignment: .topLeading) {
                                            if inviteeText.isEmpty {
                                                Text("npub1…, npub1… (comma or newline separated)")
                                                    .font(ChirpFont.mono)
                                                    .foregroundStyle(ChirpColor.textTertiary)
                                                    .allowsHitTesting(false)
                                                    .padding(.top, 8)
                                            }
                                        }
                                }
                            }
                        }
                        .padding(.horizontal, ChirpSpace.l)

                        if let errorMessage {
                            Text(errorMessage)
                                .font(ChirpFont.caption)
                                .foregroundStyle(ChirpColor.like)
                                .padding(.horizontal, ChirpSpace.l)
                        }

                        ChirpPrimaryButton(title: "Create group",
                                           systemImage: "lock.shield.fill") {
                            create()
                        }
                        .disabled(trimmedName.isEmpty || busy)
                        .opacity(trimmedName.isEmpty || busy ? 0.45 : 1.0)
                        .padding(.horizontal, ChirpSpace.l)
                        .padding(.bottom, ChirpSpace.xl)
                    }
                    .padding(.top, ChirpSpace.l)
                }
            }
            .navigationTitle("New Group")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textSecondary)
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
        VStack(alignment: .leading, spacing: ChirpSpace.xs) {
            Text(label)
                .font(ChirpFont.caption)
                .foregroundStyle(ChirpColor.textTertiary)
            TextField(placeholder, text: text)
                .font(ChirpFont.body)
                .foregroundStyle(ChirpColor.textPrimary)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
        }
    }

    private func shortNpub(_ npub: String) -> String {
        guard npub.count >= 16 else { return npub }
        return "\(npub.prefix(8))…\(npub.suffix(4))"
    }
}
