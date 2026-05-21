import SwiftUI

// OWNER: Phase-2 Agent C (Relay settings). Replace whole file.

struct RelaySettingsView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var showSheet = false
    @State private var sheetURL = ""
    @State private var sheetRole = ""
    @State private var isEditing = false

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
