import SwiftUI

// OWNER: Phase-2 Agent C (Relay settings). Replace whole file.

struct RelaySettingsView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var showSheet = false
    @State private var sheetURL = ""
    @State private var sheetRole = "both"
    @State private var isEditing = false

    private let relayRoles = ["both", "read", "write"]

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
        .background(Color(.systemBackground))
        .navigationTitle("Relays")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    openAdd()
                } label: {
                    Image(systemName: "plus")
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(ChirpColor.accent)
                }
            }
        }
        .sheet(isPresented: $showSheet) {
            RelayEditSheet(
                url: $sheetURL,
                role: $sheetRole,
                isEditing: isEditing,
                onSave: saveSheet
            )
        }
    }

    // ── Sheet helpers ─────────────────────────────────────────────────────

    private func openAdd() {
        sheetURL = ""
        sheetRole = "both"
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
}

// ── Relay config row ──────────────────────────────────────────────────────

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

            Text(relay.role.capitalized)
                .font(.system(.caption2, design: .rounded).weight(.semibold))
                .foregroundStyle(roleColor)
                .padding(.horizontal, ChirpSpace.s)
                .padding(.vertical, 3)
                .background(roleColor.opacity(0.12), in: Capsule())
        }
        .padding(.vertical, ChirpSpace.s)
        .padding(.horizontal, ChirpSpace.m)
        .background(
            Color(.secondarySystemBackground).opacity(0.6),
            in: RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall, style: .continuous)
        )
        .overlay(
            RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall, style: .continuous)
                .strokeBorder(ChirpColor.hairline, lineWidth: 1)
        )
    }

    private var roleColor: Color {
        switch relay.role {
        case "read": return Color.blue
        case "write": return ChirpColor.positive
        default: return ChirpColor.accent
        }
    }
}

// ── Add / Edit relay sheet ────────────────────────────────────────────────

private struct RelayEditSheet: View {
    @Binding var url: String
    @Binding var role: String
    let isEditing: Bool
    let onSave: () -> Void

    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    private let roles = ["both", "read", "write"]

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: ChirpSpace.xl) {
                    GlassCard {
                        VStack(alignment: .leading, spacing: ChirpSpace.m) {
                            ChirpSectionHeader(title: "Relay URL")

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
                    }
                    .padding(.horizontal, ChirpSpace.l)

                    GlassCard {
                        VStack(alignment: .leading, spacing: ChirpSpace.m) {
                            ChirpSectionHeader(title: "Role")

                            Picker("Role", selection: $role) {
                                ForEach(roles, id: \.self) { r in
                                    Text(r.capitalized).tag(r)
                                }
                            }
                            .pickerStyle(.segmented)
                        }
                    }
                    .padding(.horizontal, ChirpSpace.l)

                    ChirpPrimaryButton(
                        title: isEditing ? "Update relay" : "Add relay",
                        systemImage: isEditing ? "checkmark.circle" : "plus.circle"
                    ) {
                        onSave()
                        dismiss()
                    }
                    .disabled(trimmedURL.isEmpty)
                    .opacity(trimmedURL.isEmpty ? 0.45 : 1.0)
                    .padding(.horizontal, ChirpSpace.l)
                }
                .padding(.top, ChirpSpace.l)
            }
            .background(Color(.systemBackground))
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
