import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// GroupsView — top-level "Groups" tab root.
//
// Shows all groups in a single flat list: NIP-29 public groups and
// MLS-encrypted private groups. Encryption is a visual indicator on each
// row (lock emoji), never a section divider. No protocol vocabulary.
//
// Pending invites appear as a chip at the top when present; tapping
// navigates to InvitesView. Toolbar "+" opens NewGroupSheet.
//
// Thin-shell rule: this view is a pure render of `MarmotStore.snapshot`.
// Every label, count string, plural form, and avatar prefix crosses the
// FFI pre-formatted in `MarmotSnapshot`'s payload — Swift does no
// `.filter` / `.sorted` / `.reduce` / `RelativeDateTimeFormatter` /
// `JSONSerialization` here (chirp/AGENTS.md "canonical bad example").
//
// D6: any nil / decode failure surfaces as the empty state, never a crash.
// ─────────────────────────────────────────────────────────────────────────

struct GroupsView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var showCreate = false

    private var store: MarmotStore { model.marmot }

    var body: some View {
        groupList
            .chirpScreenBackground()
            .navigationTitle("Groups")
            .navigationBarTitleDisplayMode(.large)
            .toolbar {
                discoverButton
                createButton
            }
            .sheet(isPresented: $showCreate) {
                NewGroupSheet()
                    .environmentObject(model)
            }
    }

    // ── Unified group list ────────────────────────────────────────────────

    private var groupList: some View {
        let hasAny = !store.groups.isEmpty || store.invitesChipLabel != nil
        return Group {
            if hasAny {
                List {
                    // Pending invites chip — Rust supplies the label or nil.
                    if let invitesLabel = store.invitesChipLabel {
                        NavigationLink {
                            InvitesView()
                                .environmentObject(model)
                        } label: {
                            HStack {
                                Image(systemName: "envelope.badge.fill")
                                    .foregroundStyle(.tint)
                                Text(invitesLabel)
                                    .font(.callout.weight(.medium))
                                Spacer()
                            }
                            .padding(.vertical, 8)
                        }
                        .accessibilityIdentifier("groups-invites-chip")
                    }

                    // NIP-29 public group row
                    NavigationLink {
                        GroupChatView(store: model.groupChat)
                    } label: {
                        PublicGroupRow(
                            groupId: model.groupChat.groupId,
                            initials: model.groupChat.groupInitials)
                    }
                    .accessibilityIdentifier("nip29-group-row")
                    .accessibilityValue(model.groupChat.groupId.localId)

                    // MLS encrypted group rows
                    ForEach(store.groups) { group in
                        NavigationLink {
                            MarmotGroupChatView(group: group)
                                .environmentObject(model)
                        } label: {
                            EncryptedGroupRow(group: group)
                        }
                        .accessibilityIdentifier("marmot-group-row-\(group.idHex)")
                        .accessibilityValue(group.displayName)
                    }
                }
                .scrollContentBackground(.hidden)
            } else {
                emptyState
            }
        }
    }

    // ── Empty state ───────────────────────────────────────────────────────

    private var emptyState: some View {
        ScrollView {
            ChirpPlaceholder(
                systemImage: "person.3",
                title: "No groups yet",
                subtitle: "Create a group with friends or browse public groups."
            )
            .frame(minHeight: 360)
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

    // ── Toolbar: discover ────────────────────────────────────────────────
    //
    // Pushes `JoinGroupView` so the user can enter a NIP-29 relay URL and
    // see the public groups that relay hosts. Separate from the "+" button
    // (which creates a new MLS-encrypted group) — finding an existing
    // public group is a distinct gesture.

    @ToolbarContentBuilder
    private var discoverButton: some ToolbarContent {
        ToolbarItem(placement: .navigationBarTrailing) {
            NavigationLink {
                JoinGroupView(store: model.discoveredGroups)
            } label: {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: 17, weight: .semibold))
            }
            .accessibilityLabel("Find public groups")
            .accessibilityIdentifier("groups-discover-button")
        }
    }
}

// ── Public group row (NIP-29) ─────────────────────────────────────────────
//
// Subtitle uses # prefix to signal public/unencrypted without protocol terms.
// `initials` is the avatar-tile label — V-29 (thin-shell): the derivation
// lives in Rust (`nmp_nip29::projection::group_chat::group_initials`) and
// surfaces on every snapshot tick as `GroupChatStore.groupInitials`. The
// caller threads it in; this row binds it verbatim and never slices the
// local-id string itself.

private struct PublicGroupRow: View {
    let groupId: GroupId
    /// Rust-computed two-char uppercase avatar-tile label (V-29). The Swift
    /// derivation `String(groupId.localId.prefix(2)).uppercased()` is
    /// deliberately deleted — display formatting is Rust-owned (aim.md §2).
    let initials: String

    var body: some View {
        HStack(spacing: 8) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(.quaternary)
                    .frame(width: 40, height: 40)
                Text(initials)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.primary)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(groupId.localId)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text("# Public group")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

// ── Encrypted group row (MLS / Marmot) ───────────────────────────────────
//
// Lock emoji signals encrypted without using protocol vocabulary. EVERY
// string here comes from the Rust snapshot — no derivation in Swift.

private struct EncryptedGroupRow: View {
    let group: MarmotGroup

    var body: some View {
        HStack(spacing: 8) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(.quaternary)
                    .frame(width: 40, height: 40)
                Text(group.initials)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.primary)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(group.displayName)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text("🔒 \(group.memberCountDisplay)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            if let unreadLabel = group.unreadDisplay {
                Text(unreadLabel)
                    .font(.caption2.weight(.bold))
                    .foregroundStyle(.primary)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 3)
                    .background(.quaternary, in: Capsule())
                    .accessibilityLabel("\(unreadLabel) unread")
            }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

// ── Create-group sheet ────────────────────────────────────────────────────

struct NewGroupSheet: View {
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

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    field("Group name", text: $name, placeholder: "Trusted circle")
                    field("Description", text: $groupDescription, placeholder: "Optional")
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Members")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        TextEditor(text: $inviteeText)
                            .frame(minHeight: 90)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .overlay(alignment: .topLeading) {
                                if inviteeText.isEmpty {
                                    Text("npub1... or hex pubkey, one per line")
                                        .font(.body)
                                        .foregroundStyle(.secondary)
                                        .allowsHitTesting(false)
                                        .padding(.top, 8)
                                }
                            }
                    }
                }

                Section {
                    Picker("Type", selection: .constant(0)) {
                        Text("Private").tag(0)
                        Text("Public (coming soon)").tag(1)
                    }
                    .pickerStyle(.segmented)
                    .disabled(true)
                } footer: {
                    Text("Private groups are end-to-end encrypted. Public groups are coming soon.")
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
                        Text("Create group")
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
        Task {
            let result = await model.marmot.createGroup(
                name: trimmedName,
                description: groupDescription.trimmingCharacters(in: .whitespacesAndNewlines),
                inviteeText: inviteeText)
            busy = false
            if result.ok {
                dismiss()
            } else if let needsDisplay = result.needsDisplay, !needsDisplay.isEmpty {
                // Rust pre-abbreviated each npub; Swift only joins them.
                errorMessage = "Waiting for key packages from \(needsDisplay.joined(separator: ", "))."
            } else {
                errorMessage = result.error ?? "Could not create group"
            }
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
}
