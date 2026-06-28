import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// JoinGroupView — discover and join NIP-29 public groups on a relay.
//
// Read side: `projections["nmp.nip29.discovered_groups"]` mirrored by
// `DiscoveredGroupsStore` (lifecycle managed via
// `nmp_app_chirp_open_group_discovery` / `nmp_app_chirp_close_group_discovery`).
//
// Write side:
//   • `nmp.nip29.discover` (`KernelHandle.discoverGroups`) — pushes the
//     relay-pinned LogicalInterest for kinds 39000/39001/39002.
//   • `nmp.nip29.join` (`KernelHandle.joinGroup`) — publishes a kind:9021
//     join request, host-pinned to the group's own relay.
//
// Thin-shell rule: ZERO protocol logic here. Groups arrive alphabetically
// from the Rust `DiscoveredGroupsProjection`; this view only renders rows
// and hands the relay URL / group selection to `DiscoveredGroupsStore`.
//
// Note on "Join" UX: kind:9021 is a join *request* — the relay decides.
// We show "Requested" on the tapped row until the user dismisses the
// screen. Detecting that the request actually landed (the user's pubkey
// shows up in the relay's next kind:39002) is a follow-up that needs the
// active account's pubkey threaded into the projection.
// ─────────────────────────────────────────────────────────────────────────

struct JoinGroupView: View {
    @ObservedObject var store: DiscoveredGroupsStore
    @Environment(\.dismiss) private var dismiss

    @State private var relayUrlInput = ""

    var body: some View {
        VStack(spacing: 0) {
            relayInput
            Divider()
            content
        }
        .chirpScreenBackground()
        .navigationTitle("Find groups")
        .navigationBarTitleDisplayMode(.inline)
    }

    // ── Relay URL input ───────────────────────────────────────────────────

    private var relayInput: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Relay URL")
                .font(.caption)
                .foregroundStyle(.secondary)
            HStack(spacing: 8) {
                TextField("wss://groups.example.com", text: $relayUrlInput)
                    .font(.body)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
                    .accessibilityIdentifier("join-group-relay-input")
                    .onSubmit(submitSearch)

                Button("Search") {
                    submitSearch()
                }
                .buttonStyle(.borderedProminent)
                .disabled(trimmedRelayUrl.isEmpty)
                .accessibilityIdentifier("join-group-search-button")
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    // ── Body content (loading / empty / list) ─────────────────────────────

    @ViewBuilder
    private var content: some View {
        if store.hostRelayUrl.isEmpty {
            ScrollView {
                ChirpPlaceholder(
                    systemImage: "magnifyingglass",
                    title: "Discover groups",
                    subtitle:
                        "Enter a NIP-29 relay URL to see the public groups it hosts."
                )
                .frame(minHeight: 360)
            }
        } else if store.isSearching && store.groups.isEmpty {
            ScrollView {
                VStack(spacing: 12) {
                    ProgressView()
                    Text("Searching \(store.hostRelayUrl)…")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(minHeight: 360)
            }
        } else if store.groups.isEmpty {
            ScrollView {
                ChirpPlaceholder(
                    systemImage: "person.2.slash",
                    title: "No groups found",
                    subtitle:
                        "This relay returned no NIP-29 group metadata. Check the URL or try another relay."
                )
                .frame(minHeight: 360)
            }
        } else {
            groupList
        }
    }

    private var groupList: some View {
        List {
            Section {
                ForEach(store.groups) { group in
                    DiscoveredGroupRow(
                        group: group,
                        isJoined: store.lastJoinedGroupId == group.groupId,
                        onJoin: { store.joinGroup(group) }
                    )
                }
            } header: {
                Text(store.hostRelayUrl)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textCase(nil)
            }
        }
        .scrollContentBackground(.hidden)
        .accessibilityIdentifier("join-group-list")
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private var trimmedRelayUrl: String {
        relayUrlInput.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func submitSearch() {
        let url = trimmedRelayUrl
        guard !url.isEmpty else { return }
        store.searchGroups(relayUrl: url)
    }
}

// ── Discovered group row ──────────────────────────────────────────────────

private struct DiscoveredGroupRow: View {
    let group: DiscoveredGroup
    /// `true` after the user has tapped Join on this row (the kind:9021
    /// request was dispatched). Until the relay's next kind:39002 lands
    /// confirming the user joined, the row stays in this "Requested"
    /// state.
    let isJoined: Bool
    let onJoin: () -> Void

    // V-24 thin-shell — `initials`, `displayName`, `subtitle` arrive
    // pre-computed on `DiscoveredGroup` from `nmp-nip29`'s
    // `DiscoveredGroupsProjection`. The Swift-side derivations that
    // used to live here (string prefix + uppercase, name/groupId
    // fallback, visibility-glyph + pluralized member-count assembly)
    // are owned by Rust now.

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(.quaternary)
                    .frame(width: 44, height: 44)
                Text(group.initials)
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(.primary)
            }
            .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: 4) {
                Text(group.displayName)
                    .font(.callout.weight(.semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text(group.subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if let about = group.about, !about.isEmpty {
                    Text(about)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .padding(.top, 2)
                }
            }

            Spacer()

            Button(action: onJoin) {
                Text(isJoined ? "Requested" : "Join")
                    .font(.callout.weight(.semibold))
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(
                        isJoined ? ChirpColor.controlDisabledBackground : ChirpColor.accent)
                    .foregroundStyle(isJoined ? ChirpColor.textSecondary : ChirpColor.emphasisForeground)
                    .clipShape(Capsule())
            }
            .buttonStyle(.plain)
            .disabled(isJoined)
            .accessibilityIdentifier("join-group-join-button-\(group.groupId)")
            .accessibilityLabel(isJoined ? "Join requested" : "Join \(group.displayName)")
        }
        .padding(.vertical, 6)
        .contentShape(Rectangle())
        .accessibilityIdentifier("join-group-row-\(group.groupId)")
    }
}
