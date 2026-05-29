import SwiftUI

private enum NewGroupKind: Int, CaseIterable, Identifiable {
    case privateGroup
    case publicGroup

    var id: Int { rawValue }
    var label: String {
        switch self {
        case .privateGroup:
            return "Private"
        case .publicGroup:
            return "Public"
        }
    }
}

struct NewGroupSheet: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var kind: NewGroupKind = .privateGroup
    @State private var name = ""
    @State private var groupDescription = ""
    @State private var inviteeText = ""
    /// B1: Default relay URL for user input when creating a public NIP-29 group.
    /// This is a UI placeholder; the kernel's bootstrap relays flow through the
    /// snapshot (`relayEditRows`, `relayStatuses`). User can modify before submit.
    @State private var publicRelayUrl = "wss://relay.groups.nip29.com"
    @State private var publicLocalId = ""
    @State private var errorMessage: String?
    @State private var busy = false

    private var trimmedName: String {
        name.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    private var trimmedDescription: String {
        groupDescription.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    private var trimmedPublicRelayUrl: String {
        publicRelayUrl.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    private var trimmedPublicLocalId: String {
        publicLocalId.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        NavigationStack {
            Form {
                typeSection
                detailsSection

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
                    .disabled(createDisabled)
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

    private var typeSection: some View {
        Section {
            Picker("Type", selection: $kind) {
                ForEach(NewGroupKind.allCases) { kind in
                    Text(kind.label).tag(kind)
                }
            }
            .pickerStyle(.segmented)
        }
    }

    @ViewBuilder
    private var detailsSection: some View {
        switch kind {
        case .privateGroup:
            Section {
                field("Group name", text: $name, placeholder: "Trusted circle")
                field("Description", text: $groupDescription, placeholder: "Optional")
                membersEditor
            }
        case .publicGroup:
            Section {
                field("Group name", text: $name, placeholder: "Rust Nostr")
                field("Description", text: $groupDescription, placeholder: "Optional")
                field("Relay URL", text: $publicRelayUrl, placeholder: "wss://groups.example.com")
                    .keyboardType(.URL)
                field("Group ID", text: $publicLocalId, placeholder: "rust-nostr")
            }
        }
    }

    private var membersEditor: some View {
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

    private var createDisabled: Bool {
        if busy || trimmedName.isEmpty { return true }
        switch kind {
        case .privateGroup:
            return !model.marmot.isRegistered
        case .publicGroup:
            return trimmedPublicRelayUrl.isEmpty || trimmedPublicLocalId.isEmpty
        }
    }

    private func create() {
        busy = true
        errorMessage = nil
        switch kind {
        case .privateGroup:
            createPrivateGroup()
        case .publicGroup:
            createPublicGroup()
        }
    }

    private func createPrivateGroup() {
        Task {
            let result = await model.marmot.createGroup(
                name: trimmedName,
                description: trimmedDescription,
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

    private func createPublicGroup() {
        let group = GroupId(
            hostRelayUrl: trimmedPublicRelayUrl,
            localId: trimmedPublicLocalId)
        let result = model.createPublicGroup(
            group: group,
            name: trimmedName,
            about: trimmedDescription.isEmpty ? nil : trimmedDescription)
        busy = false
        switch result {
        case .accepted:
            dismiss()
        case .failure(let message):
            errorMessage = message
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
