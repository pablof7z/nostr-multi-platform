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
// Rust owns group membership/order and action policy; Swift renders the raw
// counts and projection-provided product labels without filtering, sorting,
// reducing, or parsing protocol payloads here.
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
            // #1651 service-init failure banner — Rust surfaces a raw machine
            // token (`initErrorKind`); the shell maps it to copy (aim.md §2).
            // A minimal diagnostic, not a recovery flow. "" = healthy (no banner).
            if let initErrorMessage = marmotInitErrorMessage(store.snapshot.initErrorKind) {
                Section {
                    HStack(spacing: ChirpSpace.s) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(ChirpColor.danger)
                        Text(initErrorMessage)
                            .font(.caption)
                            .foregroundStyle(ChirpColor.danger)
                        Spacer()
                    }
                    .padding(.vertical, ChirpSpace.xs)
                }
                .accessibilityIdentifier("groups-init-error")
            }

            // Last-op-error banner — Rust-owned (op, reason) machine codes mapped
            // to a banner by `bannerText` (aim.md §2 sanctioned mapping). Shown as
            // the first row so it is always visible regardless of scroll position.
            // Clears only when Rust next emits the snapshot with last_op_error ==
            // nil; there is no shell-side clear op (informational, not dismissible).
            if let lastErr = store.snapshot.lastOpError {
                Section {
                    HStack(spacing: ChirpSpace.s) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(ChirpColor.danger)
                        Text(lastErr.bannerText)
                            .font(.caption)
                            .foregroundStyle(ChirpColor.danger)
                        Spacer()
                    }
                    .padding(.vertical, ChirpSpace.xs)
                }
                .accessibilityIdentifier("groups-last-op-error")
            }

            // Pending ops — ops parked in Rust's deferred-completion store.
            // Each row shows the shell-computed displayLabel (aim.md §2).
            if !store.snapshot.pendingOps.isEmpty {
                Section {
                    ForEach(store.snapshot.pendingOps) { op in
                        HStack(spacing: ChirpSpace.s) {
                            ProgressView().controlSize(.small)
                            Text(op.displayLabel)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            Spacer()
                        }
                        .padding(.vertical, ChirpSpace.xs)
                        .accessibilityIdentifier("groups-pending-op-\(op.correlationId)")
                    }
                }
            }

            // Pending invites chip — shell computes the plural label or nil (aim.md §2).
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

/// #1651 — map the Rust `initErrorKind` raw token to shell-owned diagnostic
/// copy (aim.md §2: presentation lives in the shell). `nil` for `""` (healthy)
/// or any unknown token, so an older/unknown kind degrades to no banner rather
/// than misleading copy.
private func marmotInitErrorMessage(_ kind: String) -> String? {
    switch kind {
    case "keyring_unavailable":
        return "Encrypted groups unavailable: the keychain is unavailable, "
            + "so group secrets are kept in memory only and will be lost on next launch."
    case "db_key_lost":
        return "Encrypted groups unavailable: the encrypted message database key "
            + "was lost; encrypted groups are unavailable."
    case "init_failed":
        return "Encrypted groups unavailable: the encrypted message database "
            + "could not be opened. Free up space or check storage permissions, "
            + "then relaunch."
    default:
        return nil
    }
}

// ── Public group row (NIP-29) ─────────────────────────────────────────────
//
// Subtitle uses # prefix to signal public/unencrypted without protocol terms.
// `initials` is the avatar-tile label — V-29 (thin-shell): the derivation
// lives in Rust (`nmp_nip29::projection::group_events::group_initials`) and
// surfaces on every snapshot tick as `GroupChatStore.groupInitials`. The
// caller threads it in; this row binds it verbatim and never slices the
// local-id string itself.

private struct PublicGroupRow: View {
    let groupId: GroupId
    /// Projection-provided avatar-tile label for the public group row.
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
// Lock emoji signals encrypted without using protocol vocabulary.
// Raw data (name, memberCount) comes from Rust; display strings
// (initials, displayName) are shell-computed (aim.md §2).

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
