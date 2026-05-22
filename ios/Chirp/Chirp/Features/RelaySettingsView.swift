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
    /// asynchronous terminal verdict (`projections["action_results"]`)
    /// surfaces through `model.terminalActionStage(correlationId:)`.
    /// Without this seam the "Published ✓" label would lie: it would render
    /// the instant the button was tapped, even if the relay rejected the
    /// kind:10050 publish — a trust failure on the single switch that
    /// controls whether the user is reachable over NIP-17 DMs.
    @State private var publishCid: String?

    var body: some View {
        List {
            if model.relayEditRows.isEmpty {
                Section {
                    ChirpPlaceholder(
                        systemImage: "antenna.radiowaves.left.and.right",
                        title: "No relays",
                        subtitle: "Tap + to add a relay."
                    )
                    .frame(maxWidth: .infinity)
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
                }
            } else {
                Section {
                    ForEach(model.relayEditRows) { relay in
                        RelayConfigRow(relay: relay)
                            .contentShape(Rectangle())
                            .onTapGesture { openEdit(relay) }
                            .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                                Button(role: .destructive) {
                                    model.removeRelay(url: relay.url)
                                } label: {
                                    Label("Remove", systemImage: "trash")
                                }
                            }
                            .listRowBackground(Color.clear)
                            .listRowSeparator(.hidden)
                    }
                } header: {
                    ChirpSectionHeader(title: "Configured relays")
                        .padding(.bottom, ChirpSpace.xs)
                }
            }

            Section {
                Text("Advertises your relays as DM inbox so others can reach you via NIP-17.")
                    .font(.system(.footnote, design: .rounded))
                    .foregroundStyle(ChirpColor.textSecondary)
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)

                dmInboxPublishRow
                    .listRowBackground(Color.clear)
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
        if let stage = publishCid.flatMap({ model.terminalActionStage(correlationId: $0)?.stage }) {
            switch stage {
            case .accepted:
                Text("Published ✓")
                    .font(.system(.subheadline, design: .rounded).weight(.semibold))
                    .foregroundStyle(ChirpColor.positive)
                    .padding(.vertical, ChirpSpace.s)
            case let .failed(reason):
                VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                    Text("Publish failed")
                        .font(.system(.subheadline, design: .rounded).weight(.semibold))
                        .foregroundStyle(Color.red)
                    if !reason.isEmpty {
                        Text(reason)
                            .font(.system(.footnote, design: .rounded))
                            .foregroundStyle(ChirpColor.textSecondary)
                    }
                    publishButton(label: "Try again", systemImage: "arrow.clockwise")
                }
                .padding(.vertical, ChirpSpace.s)
            case .requested, .awaitingCapability, .publishing, .unknown(_):
                // A non-terminal stage surfaced here is unusual (the snapshot
                // mirror only feeds terminals into `terminalActionStage`),
                // but defensively render it as the in-flight spinner so the
                // user never sees an empty row.
                publishingRow
            }
        } else if publishCid != nil {
            // Correlation id stashed, but no terminal has landed yet — the
            // verdict is still in flight (in-flight publish, or a fast next
            // snapshot that overwrote `actionStages` before SwiftUI re-read).
            publishingRow
        } else {
            publishButton(label: "Publish as DM inboxes", systemImage: "tray.and.arrow.up")
        }
    }

    private var publishingRow: some View {
        HStack(spacing: ChirpSpace.s) {
            ProgressView().controlSize(.small)
            Text("Publishing…")
                .font(.system(.subheadline, design: .rounded))
                .foregroundStyle(ChirpColor.textSecondary)
        }
        .padding(.vertical, ChirpSpace.s)
    }

    private func publishButton(label: String, systemImage: String) -> some View {
        Button {
            let result = model.publishDmRelayList(relays: model.relayEditRows.map(\.url))
            // PR-A: only stash a correlation id on accept — a synchronous
            // dispatch rejection has already routed through `track()` into
            // `lastDispatchError` (the global toast slot). Clearing
            // `publishCid` here resets the row to the button so the user
            // can retry without first observing a stale terminal.
            publishCid = result.correlationId
        } label: {
            Label(label, systemImage: systemImage)
        }
        .disabled(model.relayEditRows.isEmpty)
    }

    private func openAdd() {
        sheetURL = ""
        sheetRole = defaultRelayRole
        isEditing = false
        showSheet = true
    }

    private func openEdit(_ relay: RelayEditRow) {
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

private struct RelayConfigRow: View {
    let relay: RelayEditRow

    var body: some View {
        HStack(spacing: ChirpSpace.m) {
            Image(systemName: "antenna.radiowaves.left.and.right")
                .foregroundStyle(roleColor)
                .font(.system(size: 14, weight: .medium))
                .frame(width: 20)

            VStack(alignment: .leading, spacing: 2) {
                Text(relay.url)
                    .font(ChirpFont.mono)
                    .foregroundStyle(ChirpColor.textPrimary)
                    .lineLimit(1)
            }

            Spacer()

            Text(relay.roleLabel)
                .font(.system(.caption2, design: .rounded).weight(.semibold))
                .foregroundStyle(roleColor)
                .padding(.horizontal, ChirpSpace.s)
                .padding(.vertical, 3)
                .background(roleColor.opacity(0.12), in: Capsule())
        }
        .padding(.vertical, ChirpSpace.s)
    }

    private var roleColor: Color {
        relayRoleTint(relay.roleTint)
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

private func relayRoleTint(_ tint: String) -> Color {
    switch tint {
    case "info":
        return .cyan
    case "success":
        return ChirpColor.positive
    case "neutral":
        return ChirpColor.textSecondary
    default:
        return ChirpColor.accent
    }
}
