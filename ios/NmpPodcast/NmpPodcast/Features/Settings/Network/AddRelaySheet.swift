import SwiftUI

/// Sheet for adding a new relay. URL field with light validation (wss:// or
/// ws:// prefix) + Read/Write toggles. The kernel canonicalises and persists
/// the URL — this sheet only collects input.
struct AddRelaySheet: View {
    @Environment(\.dismiss) private var dismiss

    /// Called with the trimmed URL + role flags when the user taps Add.
    let onAdd: (_ url: String, _ read: Bool, _ write: Bool) -> Void

    @State private var urlText = ""
    @State private var read = true
    @State private var write = true

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("wss://relay.example.com", text: $urlText)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    if isUnencrypted {
                        Label("Unencrypted connection — use wss:// when possible.", systemImage: "exclamationmark.triangle")
                            .font(.caption)
                            .foregroundStyle(.orange)
                    }
                    if let paste = clipboardURL, paste != urlText {
                        Button {
                            urlText = paste
                        } label: {
                            HStack {
                                Image(systemName: "doc.on.clipboard")
                                Text("Paste \(paste)")
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                            }
                            .font(.caption)
                        }
                    }
                } header: {
                    Text("Relay URL")
                } footer: {
                    Text("Use wss:// for a secure connection.")
                }

                Section {
                    Toggle("Read", isOn: $read)
                    Toggle("Write", isOn: $write)
                } header: {
                    Text("Roles")
                } footer: {
                    Text("Read pulls events from this relay. Write publishes your events here. Disabling both is not allowed.")
                }
            }
            .navigationTitle("Add Relay")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Add") {
                        let trimmed = urlText.trimmingCharacters(in: .whitespaces)
                        // Guard against the both-off state from the toggles.
                        let r = read || !write
                        onAdd(trimmed, r, write)
                        dismiss()
                    }
                    .disabled(!isValid || (!read && !write))
                }
            }
        }
    }

    private var isValid: Bool {
        let trimmed = urlText.trimmingCharacters(in: .whitespaces)
        return trimmed.hasPrefix("wss://") || trimmed.hasPrefix("ws://")
    }

    private var isUnencrypted: Bool {
        urlText.trimmingCharacters(in: .whitespaces).hasPrefix("ws://")
    }

    private var clipboardURL: String? {
        guard let s = UIPasteboard.general.string?.trimmingCharacters(in: .whitespaces) else {
            return nil
        }
        guard s.hasPrefix("wss://") || s.hasPrefix("ws://") else { return nil }
        return s
    }
}
