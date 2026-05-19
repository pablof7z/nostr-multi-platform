import SwiftUI

/// Sheet for adding a new relay. URL field + role chips. Sane defaults:
/// Read + Write on, Rooms and Indexer off. A user can tap chips after the
/// relay is in the list if they want to change the roles.
struct AddRelaySheet: View {
    @Environment(HighlighterStore.self) private var appStore
    @Environment(\.dismiss) private var dismiss

    let onAdd: (RelayConfig) -> Void

    @State private var urlText = ""
    @State private var read = true
    @State private var write = true
    @State private var rooms = false
    @State private var indexer = false

    /// NIP-11 probe status. Populated after the URL field loses focus (or
    /// after a 600ms debounce) so the user sees what relay they're about
    /// to add without the probe firing on every keystroke.
    @State private var probeResult: Nip11Document?
    @State private var probeError: String?
    @State private var probeInFlight = false
    @State private var debounceTask: Task<Void, Never>?

    /// Whether the URL looks like a wss:// or ws:// URL.
    private var isValid: Bool {
        let trimmed = urlText.trimmingCharacters(in: .whitespaces)
        return trimmed.hasPrefix("wss://") || trimmed.hasPrefix("ws://")
    }

    private var isUnencrypted: Bool {
        urlText.trimmingCharacters(in: .whitespaces).hasPrefix("ws://")
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("wss://relay.example.com", text: $urlText)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .onChange(of: urlText) { _, _ in scheduleProbe() }
                    if isUnencrypted {
                        Label("Unencrypted connection — use wss:// when possible.", systemImage: "exclamationmark.triangle")
                            .font(.caption)
                            .foregroundStyle(.orange)
                    }
                    if let paste = clipboardURL, paste != urlText {
                        Button {
                            urlText = paste
                            scheduleProbe()
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
                    probeStatus
                } header: {
                    Text("Relay URL")
                } footer: {
                    Text("Use wss:// for a secure connection.")
                }

                Section {
                    Toggle("Read", isOn: $read)
                    Toggle("Write", isOn: $write)
                    Toggle("Rooms", isOn: $rooms)
                    Toggle("Indexer", isOn: $indexer)
                } header: {
                    Text("Roles")
                } footer: {
                    Text("Read/Write affect the kind:10002 event your app publishes. Rooms routes NIP-29 group traffic. Indexer is the outbox-model bootstrap pool.")
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
                        onAdd(
                            RelayConfig(
                                url: trimmed,
                                read: read,
                                write: write,
                                rooms: rooms,
                                indexer: indexer
                            )
                        )
                        dismiss()
                    }
                    .disabled(!isValid)
                }
            }
        }
    }

    /// Returns the clipboard string if and only if it looks like a wss URL.
    /// Avoids noisy paste prompts for arbitrary text.
    private var clipboardURL: String? {
        guard let s = UIPasteboard.general.string?.trimmingCharacters(in: .whitespaces) else {
            return nil
        }
        guard s.hasPrefix("wss://") || s.hasPrefix("ws://") else { return nil }
        return s
    }

    // MARK: - NIP-11 probe

    /// Inline status line below the URL field. Shows the fetched relay
    /// software / name after a successful probe, a muted note while the
    /// probe is in flight, or a gentle warning if the probe failed.
    /// Probe failure never blocks Add — relays go up and down all the
    /// time.
    @ViewBuilder
    private var probeStatus: some View {
        if probeInFlight {
            HStack(spacing: 6) {
                ProgressView().scaleEffect(0.7)
                Text("Checking relay…")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        } else if let doc = probeResult {
            HStack(spacing: 6) {
                Image(systemName: "checkmark.seal.fill")
                    .foregroundStyle(.green)
                Text(nip11Summary(doc))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        } else if let err = probeError {
            HStack(spacing: 6) {
                Image(systemName: "questionmark.circle")
                    .foregroundStyle(.secondary)
                Text(err)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func nip11Summary(_ doc: Nip11Document) -> String {
        let softwareLabel: String? = doc.software.map { name in
            if let version = doc.version {
                return "\(name) \(version)"
            }
            return name
        }
        let parts: [String?] = [
            doc.name,
            softwareLabel,
            doc.supportedNips.isEmpty ? nil : "\(doc.supportedNips.count) NIPs",
        ]
        let joined = parts.compactMap { $0 }.joined(separator: " • ")
        return joined.isEmpty ? "Reachable (no NIP-11 metadata)" : joined
    }

    /// Debounce the probe so it runs at most once per 600ms of idle typing.
    /// Cancels an in-flight probe if the URL changes before it resolves.
    private func scheduleProbe() {
        debounceTask?.cancel()
        probeResult = nil
        probeError = nil
        guard isValid else { return }
        let url = urlText.trimmingCharacters(in: .whitespaces)
        let core = appStore.safeCore
        debounceTask = Task { [url] in
            try? await Task.sleep(for: .milliseconds(600))
            guard !Task.isCancelled else { return }
            probeInFlight = true
            defer { probeInFlight = false }
            do {
                let doc = try await core.probeRelayNip11(url)
                guard !Task.isCancelled else { return }
                probeResult = doc
                probeError = nil
            } catch {
                guard !Task.isCancelled else { return }
                probeResult = nil
                probeError = "Couldn't reach the relay — you can still add it."
            }
        }
    }
}
