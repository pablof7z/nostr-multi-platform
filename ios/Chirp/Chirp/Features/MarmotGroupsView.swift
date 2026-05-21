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
// Thin-shell rule: ZERO protocol logic here. All ordering and state live
// in Rust; this view only renders snapshots and navigates.
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
                createButton
            }
            .sheet(isPresented: $showCreate) {
                NewGroupSheet()
                    .environmentObject(model)
            }
    }

    // ── Unified group list ────────────────────────────────────────────────

    private var groupList: some View {
        let hasAny = !store.groups.isEmpty || !store.pendingWelcomes.isEmpty
        return Group {
            if hasAny {
                List {
                    // Pending invites chip — shown only when invites exist
                    if !store.pendingWelcomes.isEmpty {
                        NavigationLink {
                            InvitesView()
                                .environmentObject(model)
                        } label: {
                            HStack {
                                Image(systemName: "envelope.badge.fill")
                                    .foregroundStyle(.tint)
                                Text(
                                    store.pendingWelcomes.count == 1
                                        ? "1 invite"
                                        : "\(store.pendingWelcomes.count) invites"
                                )
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
                        PublicGroupRow(groupId: model.groupChat.groupId)
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
                        .accessibilityValue(group.name)
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
}

// ── Public group row (NIP-29) ─────────────────────────────────────────────
//
// Subtitle uses # prefix to signal public/unencrypted without protocol terms.

private struct PublicGroupRow: View {
    let groupId: GroupId

    private var initials: String {
        let id = groupId.localId
        guard !id.isEmpty else { return "?" }
        return String(id.prefix(2)).uppercased()
    }

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
// Lock emoji signals encrypted without using protocol vocabulary.

private struct EncryptedGroupRow: View {
    let group: MarmotGroup

    private var initials: String {
        let n = group.name.isEmpty ? "?" : group.name
        return String(n.prefix(2)).uppercased()
    }

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
                Text(group.name.isEmpty ? "Untitled group" : group.name)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text("🔒 \(group.members.count) member\(group.members.count == 1 ? "" : "s")")
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
        let result = model.marmot.createGroup(
            name: trimmedName,
            description: groupDescription.trimmingCharacters(in: .whitespacesAndNewlines),
            inviteeNpubs: invitees)
        busy = false
        if result.ok {
            dismiss()
        } else if let needs = result.needs, !needs.isEmpty {
            errorMessage = "Waiting for key packages from \(needs.map(shortNpub).joined(separator: ", "))."
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
