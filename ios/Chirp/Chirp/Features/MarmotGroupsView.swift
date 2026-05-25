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
        List {
            // Pending invites chip — Rust supplies the label or nil.
            if let invitesLabel = store.invitesChipLabel {
                NavigationLink {
                    InvitesView()
                        .environmentObject(model)
                } label: {
                    HStack {
                        Image(systemName: "envelope.badge.fill")
                            .foregroundStyle(ChirpColor.accent)
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
            .disabled(!model.hasActiveAccount)
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
                // ADR-0032: pluralisation lives in the presentation layer.
                Text("🔒 \(group.memberCount) \(group.memberCount == 1 ? "member" : "members")")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            if let unread = group.unreadCount, unread > 0 {
                Text("\(unread)")
                    .font(.caption2.weight(.bold))
                    .foregroundStyle(.primary)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 3)
                    .background(.quaternary, in: Capsule())
                    .accessibilityLabel("\(unread) unread")
            }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}
