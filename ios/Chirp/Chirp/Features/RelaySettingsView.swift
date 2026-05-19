import SwiftUI

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
                    ContentUnavailableView(
                        "No relays",
                        systemImage: "antenna.radiowaves.left.and.right",
                        description: Text("Tap + to add a relay.")
                    )
                }
            } else {
                Section("Configured relays") {
                    ForEach(model.relayEditRows) { relay in
                        HStack {
                            Text(relay.url)
                                .font(.callout.monospaced())
                                .lineLimit(1)
                            Spacer()
                            Text(relay.role.capitalized)
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(roleColor(relay.role))
                                .padding(.horizontal, 8)
                                .padding(.vertical, 3)
                                .background(roleColor(relay.role).opacity(0.12), in: Capsule())
                        }
                        .contentShape(Rectangle())
                        .onTapGesture { openEdit(relay) }
                        .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                            Button(role: .destructive) {
                                model.removeRelay(url: relay.url)
                            } label: {
                                Label("Remove", systemImage: "trash")
                            }
                        }
                    }
                }
            }
        }
        .navigationTitle("Relays")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    openAdd()
                } label: {
                    Image(systemName: "plus")
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

    private func roleColor(_ role: String) -> Color {
        switch role {
        case "read": return .accentColor
        case "write": return .green
        default: return .accentColor
        }
    }
}

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
            Form {
                Section("Relay URL") {
                    TextField("wss://relay.example.com", text: $url)
                        .font(.callout.monospaced())
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                        .disabled(isEditing)
                }

                Section("Role") {
                    Picker("Role", selection: $role) {
                        ForEach(roles, id: \.self) { r in
                            Text(r.capitalized).tag(r)
                        }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                }
            }
            .navigationTitle(isEditing ? "Edit Relay" : "Add Relay")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        onSave()
                        dismiss()
                    } label: {
                        Text(isEditing ? "Update" : "Add")
                    }
                    .disabled(trimmedURL.isEmpty)
                }
            }
        }
    }

    private var trimmedURL: String {
        url.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
