import SwiftUI

// OWNER: Phase-2 Agent C (Relay settings). Replace whole file.

struct RelaySettingsView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var showSheet = false
    @State private var sheetURL = ""
    @State private var sheetRole = ""
    @State private var isEditing = false
    /// Correlation id minted by `dispatch_action("nmp.nip17.publish_relay_list", …)`
    /// — set on tap, cleared either when the user re-publishes or when the
    /// asynchronous terminal verdict surfaces through
    /// `model.recentTerminal(correlationId:)` (V5: the kernel's
    /// `action_lifecycle` projection owns the lifecycle bookkeeping).
    /// Without this seam the "Published ✓" label would lie: it would render
    /// the instant the button was tapped, even if the relay rejected the
    /// kind:10050 publish — a trust failure on the single switch that
    /// controls whether the user is reachable over NIP-17 DMs.
    @State private var publishCid: String?

    var body: some View {
        List {
            if model.configuredRelays.isEmpty {
                Section {
                    ChirpPlaceholder(
                        systemImage: "antenna.radiowaves.left.and.right",
                        title: "No relays",
                        subtitle: "Tap + to add a relay."
                    )
                    .frame(maxWidth: .infinity)
                    .listRowBackground(ChirpColor.transparent)
                    .listRowSeparator(.hidden)
                }
            } else {
                Section {
                    ForEach(model.configuredRelays) { relay in
                        RelayConfigRow(relay: relay, relayRoleOptions: model.relayRoleOptions)
                            .contentShape(Rectangle())
                            .onTapGesture { openEdit(relay) }
                            .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                                Button(role: .destructive) {
                                    model.removeRelay(url: relay.url)
                                } label: {
                                    Label("Remove", systemImage: "trash")
                                }
                            }
                            .listRowBackground(ChirpColor.transparent)
                            .listRowSeparator(.hidden)
                    }
                } header: {
                    ChirpSectionHeader(title: "Configured relays")
                        .padding(.bottom, ChirpSpace.xs)
                }
            }

            Section {
                Text("Advertises your relays as DM inbox so others can reach you via NIP-17.")
                    .font(.footnote)
                    .foregroundStyle(ChirpColor.textSecondary)
                    .listRowBackground(ChirpColor.transparent)
                    .listRowSeparator(.hidden)

                dmInboxPublishRow
                    .listRowBackground(ChirpColor.transparent)
                    .listRowSeparator(.hidden)
            } header: {
                ChirpSectionHeader(title: "DM inbox")
                    .padding(.bottom, ChirpSpace.xs)
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .chirpScreenBackground()
        .navigationTitle("Relays")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    openAdd()
                } label: {
                    Image(systemName: "plus")
                        .font(.system(size: 17, weight: .semibold))
                }
            }
        }
        .sheet(isPresented: $showSheet) {
            RelayEditSheet(
                url: $sheetURL,
                role: $sheetRole,
                isEditing: isEditing,
                relayRoles: model.relayRoleOptions,
                onSave: saveSheet
            )
        }
    }

    /// The button / status row for "publish kind:10050 DM-inbox relay list".
    ///
    /// State machine — driven entirely from the kernel's `action_results`
    /// terminal verdict, NEVER from a same-tap boolean:
    ///
    ///   * no `publishCid`                     → "Publish as DM inboxes" button
    ///   * `publishCid` set, no terminal yet   → "Publishing…" (disabled spinner row)
    ///   * terminal `.accepted`                → "Published ✓"
    ///   * terminal `.failed(reason)`          → red error + button re-enabled
    @ViewBuilder
    private var dmInboxPublishRow: some View {
        if let stage = publishCid.flatMap({ model.recentTerminal(correlationId: $0)?.stage }) {
            switch stage {
            case .accepted:
                Text("Published ✓")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(ChirpColor.positive)
                    .padding(.vertical, ChirpSpace.s)
            case .failed:
                // #1735: localize the reason_code, falling back to the English
                // prose `reason` the wire carries.
                let reason = stage.localizedReason ?? ""
                VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                    Text("Publish failed")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(ChirpColor.danger)
                    if !reason.isEmpty {
                        Text(reason)
                            .font(.footnote)
                            .foregroundStyle(ChirpColor.textSecondary)
                    }
                    publishButton(label: "Try again", systemImage: "arrow.clockwise")
                }
                .padding(.vertical, ChirpSpace.s)
            case .cancelled:
                // S7/#1754: user-initiated cancellation — a DISTINCT terminal,
                // NOT a failure. Re-enable the publish button with a neutral
                // label (no error treatment).
                VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                    Text("Publish cancelled")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(ChirpColor.textSecondary)
                    publishButton(label: "Try again", systemImage: "arrow.clockwise")
                }
                .padding(.vertical, ChirpSpace.s)
            case .requested, .awaitingCapability, .publishing, .unknown(_):
                // V5: `recentTerminal` only returns terminal entries, so this
                // arm is theoretically unreachable. Render the in-flight
                // spinner defensively so the user never sees an empty row.
                publishingRow
            }
        } else if publishCid != nil {
            // V5: correlation id stashed, but no terminal has landed yet —
            // the action is either still in flight (kernel's
            // `action_lifecycle.inFlight`) or its terminal has already
            // dropped past the 3-second TTL. Render the spinner either way;
            // the next user action publishes a fresh correlation id.
            publishingRow
        } else {
            publishButton(label: "Publish as DM inboxes", systemImage: "tray.and.arrow.up")
        }
    }

    private var publishingRow: some View {
        HStack(spacing: ChirpSpace.s) {
            ProgressView().controlSize(.small)
            Text("Publishing…")
                .font(.subheadline)
                .foregroundStyle(ChirpColor.textSecondary)
        }
        .padding(.vertical, ChirpSpace.s)
    }

    private func publishButton(label: String, systemImage: String) -> some View {
        Button {
            let result = model.publishDmRelayList(relays: model.configuredRelays.map(\.url))
            // Also publish NIP-65 kind:10002 (general relay list) so other
            // Nostr clients can discover this user's read/write relays via
            // NIP-65 relay discovery. Rust registers and owns
            // `nmp.nip65.publish_relay_list`, including the indexer-role
            // policy for rows that cannot be represented in kind:10002.
            model.publishRelayList(relays: model.configuredRelays)
            // PR-A: only stash a correlation id on accept — a synchronous
            // dispatch rejection has already routed through `track()` into
            // `lastDispatchError` (the global toast slot). Clearing
            // `publishCid` here resets the row to the button so the user
            // can retry without first observing a stale terminal. The
            // correlation id tracked is the kind:10050 (DM inbox) one —
            // the kind:10002 dispatch's correlation id is intentionally
            // dropped (the user-facing spinner is keyed on the DM inbox
            // publish; both dispatches share the same accept/reject path
            // through `track()`, so a kind:10002 rejection still surfaces
            // through `lastDispatchError`).
            publishCid = result.correlationId
        } label: {
            Label(label, systemImage: systemImage)
        }
        .disabled(model.configuredRelays.isEmpty)
    }

    private func openAdd() {
        sheetURL = ""
        sheetRole = defaultRelayRole
        isEditing = false
        showSheet = true
    }

    private func openEdit(_ relay: AppRelay) {
        sheetURL = relay.url
        sheetRole = relay.role
        isEditing = true
        showSheet = true
    }

    private func saveSheet() {
        let url = sheetURL.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !url.isEmpty else { return }
        model.addRelay(url: url, role: sheetRole)
    }

    private var defaultRelayRole: String {
        model.relayRoleOptions.first(where: { $0.isDefault })?.value
            ?? model.relayRoleOptions.first?.value
            ?? ""
    }
}

/// A configured-relay row in Chirp's relay settings.
///
/// Thin shell over the shared `NostrRelayRow` gallery primitive (issue #996):
/// it resolves the relay's role to the kernel-emitted `label` + `tint` token
/// from `model.relayRoleOptions` (the same `relay_role_options` projection the
/// gallery consumes) and hands those straight to the component. No role→label
/// / role→tint derivation lives in Swift; the only presentation logic is the
/// token→Color mapping owned by `NostrRelayRow.tintColor(for:)`.
private struct RelayConfigRow: View {
    let relay: AppRelay
    let relayRoleOptions: [RelayRoleOption]

    private var roleOption: RelayRoleOption? {
        relayRoleOptions.first { $0.value == relay.role }
    }

    var body: some View {
        NostrRelayRow(
            url: relay.url,
            roleLabel: roleOption?.label ?? relay.role,
            roleTint: roleOption?.tint ?? "accent"
        )
    }
}

private struct RelayEditSheet: View {
    @Binding var url: String
    @Binding var role: String
    let isEditing: Bool
    let relayRoles: [RelayRoleOption]
    let onSave: () -> Void

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section("Relay URL") {
                    HStack(spacing: ChirpSpace.s) {
                        Image(systemName: "antenna.radiowaves.left.and.right")
                            .foregroundStyle(ChirpColor.accent)
                            .font(.system(size: 15))
                        TextField("wss://relay.example.com", text: $url)
                            .font(ChirpFont.mono)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .keyboardType(.URL)
                            .disabled(isEditing)
                    }
                }

                Section("Role") {
                    Picker("Role", selection: $role) {
                        ForEach(relayRoles) { relayRole in
                            Text(relayRole.label).tag(relayRole.value)
                        }
                    }
                    .pickerStyle(.segmented)
                }

                Section {
                    Button {
                        onSave()
                        dismiss()
                    } label: {
                        Label(
                            isEditing ? "Update relay" : "Add relay",
                            systemImage: isEditing ? "checkmark.circle" : "plus.circle"
                        )
                    }
                    .disabled(trimmedURL.isEmpty || role.isEmpty)
                }
            }
            .scrollContentBackground(.hidden)
            .chirpScreenBackground()
            .navigationTitle(isEditing ? "Edit Relay" : "Add Relay")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                        .foregroundStyle(ChirpColor.textSecondary)
                }
            }
        }
    }

    private var trimmedURL: String {
        url.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
