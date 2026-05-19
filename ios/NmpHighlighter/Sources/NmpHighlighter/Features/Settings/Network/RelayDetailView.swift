import SwiftUI

/// Detail screen for a single relay. Big state header + cumulative traffic +
/// role toggles + Remove action.
struct RelayDetailView: View {
    let url: String
    let store: NetworkSettingsStore

    @Environment(HighlighterStore.self) private var appStore
    @Environment(\.dismiss) private var dismiss
    @State private var showRemoveConfirm = false
    @State private var isSaving = false

    /// Names of joined rooms hosted on this relay. Non-empty → the user
    /// would lose access to those rooms if they remove the relay. Listed
    /// in the confirmation dialog so the user can decide.
    private var orphanedRoomNames: [String] {
        appStore.joinedCommunities
            .filter {
                $0.relayUrl.trimmingCharacters(in: .whitespaces)
                    == url.trimmingCharacters(in: .whitespaces)
            }
            .map { $0.name.isEmpty ? $0.id : $0.name }
    }

    var body: some View {
        List {
            headerSection
            orphanRoomsSection
            statsSection
            rolesSection
            removeSection
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Relay")
        .navigationBarTitleDisplayMode(.inline)
        .confirmationDialog(
            orphanedRoomNames.isEmpty
                ? "Remove this relay?"
                : "Remove — you're a member of rooms here",
            isPresented: $showRemoveConfirm,
            titleVisibility: .visible
        ) {
            Button("Remove", role: .destructive) {
                Task {
                    await store.remove(url)
                    dismiss()
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            if orphanedRoomNames.isEmpty {
                Text("Highlighter will stop sending and receiving events through this relay.")
            } else {
                Text("This relay hosts \(orphanedRoomNames.count) of your rooms (\(orphanedRoomNames.prefix(3).joined(separator: ", "))\(orphanedRoomNames.count > 3 ? ", …" : "")). Removing it will cut you off from them until you re-add it.")
            }
        }
    }

    // MARK: - Sections

    private var config: RelayConfig? {
        store.relays.first(where: { $0.url == url })
    }

    private var diagnostic: RelayDiagnostic? {
        store.diagnostic(for: url)
    }

    private var nip11: Nip11Document? { store.nip11(for: url) }

    @ViewBuilder
    private var headerSection: some View {
        Section {
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .top, spacing: 12) {
                    RelayAvatar(url: url, nip11: nip11, size: 52)
                    VStack(alignment: .leading, spacing: 2) {
                        if let name = nip11?.name?.trimmingCharacters(in: .whitespaces), !name.isEmpty {
                            Text(name).font(.title3.weight(.semibold))
                        }
                        Text(url)
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                            .truncationMode(.middle)
                        if let desc = nip11?.description?.trimmingCharacters(in: .whitespaces), !desc.isEmpty {
                            Text(desc)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(3)
                                .padding(.top, 2)
                        }
                    }
                }
                HStack(spacing: 8) {
                    stateDot
                    Text(stateLabel).font(.subheadline.weight(.medium))
                    Spacer()
                    if let rtt = diagnostic?.rttMs {
                        Text("\(rtt) ms")
                            .font(.subheadline.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .padding(.vertical, 4)
        }
    }

    @ViewBuilder
    private var statsSection: some View {
        if let d = diagnostic {
            Section("Traffic") {
                LabeledContent("Received", value: formatBytes(d.bytesReceived))
                LabeledContent("Sent", value: formatBytes(d.bytesSent))
                if let since = d.connectedSinceTs {
                    LabeledContent(
                        "Connected since",
                        value: formatUnixSeconds(since)
                    )
                }
            }
        }
    }

    @ViewBuilder
    private var rolesSection: some View {
        if let cfg = config {
            Section {
                ToggleRow(label: "Read", isOn: cfg.read) { on in
                    Task { await applyRoles(cfg, read: on) }
                }
                ToggleRow(label: "Write", isOn: cfg.write) { on in
                    Task { await applyRoles(cfg, write: on) }
                }
                ToggleRow(label: "Rooms", isOn: cfg.rooms) { on in
                    Task { await applyRoles(cfg, rooms: on) }
                }
                ToggleRow(label: "Indexer", isOn: cfg.indexer) { on in
                    Task { await applyRoles(cfg, indexer: on) }
                }
            } header: {
                Text("Roles")
            } footer: {
                Text("Changing Read or Write republishes your kind:10002 relay list.")
            }
        }
    }

    private var removeSection: some View {
        Section {
            Button(role: .destructive) {
                showRemoveConfirm = true
            } label: {
                HStack {
                    Spacer()
                    Text("Remove Relay").fontWeight(.semibold)
                    Spacer()
                }
            }
            .disabled(isSaving)
        }
    }

    @ViewBuilder
    private var orphanRoomsSection: some View {
        if !orphanedRoomNames.isEmpty {
            Section {
                VStack(alignment: .leading, spacing: 4) {
                    Label("Hosts your rooms", systemImage: "person.3.fill")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.orange)
                    Text(orphanedRoomNames.prefix(5).joined(separator: ", ") + (orphanedRoomNames.count > 5 ? ", …" : ""))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 2)
            } footer: {
                Text("Removing this relay will cut you off from these rooms until you re-add it or leave them.")
            }
        }
    }

    // MARK: - State pieces

    @ViewBuilder
    private var stateDot: some View {
        let color: Color = {
            switch diagnostic?.state {
            case .connected: return .green
            case .connecting: return .yellow
            case .disconnected, .terminated, .banned: return .red
            case .none: return .gray
            }
        }()
        Circle().fill(color).frame(width: 12, height: 12)
    }

    private var stateLabel: String {
        switch diagnostic?.state {
        case .connected: return "Connected"
        case .connecting: return "Connecting…"
        case .disconnected: return "Disconnected"
        case .terminated: return "Terminated"
        case .banned: return "Banned"
        case .none: return "Unknown"
        }
    }

    // MARK: - Actions

    private func applyRoles(
        _ cfg: RelayConfig,
        read: Bool? = nil,
        write: Bool? = nil,
        rooms: Bool? = nil,
        indexer: Bool? = nil
    ) async {
        isSaving = true
        defer { isSaving = false }
        await store.setRoles(
            url: cfg.url,
            read: read ?? cfg.read,
            write: write ?? cfg.write,
            rooms: rooms ?? cfg.rooms,
            indexer: indexer ?? cfg.indexer
        )
    }

    // MARK: - Formatting

    private func formatBytes(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .binary)
    }

    private func formatUnixSeconds(_ seconds: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        let f = DateFormatter()
        f.dateStyle = .short
        f.timeStyle = .short
        return f.string(from: date)
    }
}

/// Thin Toggle wrapper that notifies on commit only — avoids firing the
/// async save for each interim state during a drag.
private struct ToggleRow: View {
    let label: String
    let isOn: Bool
    let onChange: (Bool) -> Void

    @State private var localValue: Bool = false
    @State private var didInit = false

    var body: some View {
        Toggle(label, isOn: Binding(
            get: { didInit ? localValue : isOn },
            set: { newValue in
                localValue = newValue
                didInit = true
                onChange(newValue)
            }
        ))
        .onChange(of: isOn) { _, newValue in
            // Source-of-truth sync after the parent reloads from Rust.
            localValue = newValue
            didInit = true
        }
    }
}
