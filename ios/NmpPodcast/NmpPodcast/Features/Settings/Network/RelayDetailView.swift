import SwiftUI

/// Detail screen for a single relay. State header + cumulative traffic +
/// Read/Write role toggles + Remove with confirmation. Reads from the
/// kernel snapshot via `KernelModel`; toggle commits are routed back through
/// `setRelayRoles` which the kernel upserts in place.
struct RelayDetailView: View {
    let url: String

    @EnvironmentObject private var kernelModel: KernelModel
    @Environment(\.dismiss) private var dismiss
    @State private var showRemoveConfirm = false

    var body: some View {
        List {
            headerSection
            if let s = status {
                statsSection(s)
            }
            if let cfg = relay {
                rolesSection(cfg)
            }
            removeSection
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Relay")
        .navigationBarTitleDisplayMode(.inline)
        .confirmationDialog(
            "Remove this relay?",
            isPresented: $showRemoveConfirm,
            titleVisibility: .visible
        ) {
            Button("Remove", role: .destructive) {
                kernelModel.removeRelay(url: url)
                dismiss()
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Podcastr will stop sending and receiving events through this relay.")
        }
    }

    // MARK: - Derived

    private var relay: RelayEditRow? {
        kernelModel.relays.first(where: { $0.url == url })
    }

    private var status: RelayKernelStatus? {
        kernelModel.status(for: url)
    }

    // MARK: - Sections

    private var headerSection: some View {
        Section {
            VStack(alignment: .leading, spacing: 10) {
                Text(url)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .truncationMode(.middle)
                HStack(spacing: 8) {
                    Circle().fill(stateColor).frame(width: 12, height: 12)
                    Text(stateLabel).font(.subheadline.weight(.medium))
                    Spacer()
                    if let count = status?.reconnectCount, count > 0 {
                        Text("\(count) reconnect\(count == 1 ? "" : "s")")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                }
                if let err = status?.lastError, !err.isEmpty {
                    Text(err)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            .padding(.vertical, 4)
        }
    }

    @ViewBuilder
    private func statsSection(_ s: RelayKernelStatus) -> some View {
        Section("Traffic") {
            LabeledContent("Received", value: formatBytes(s.bytesRx ?? 0))
            LabeledContent("Sent", value: formatBytes(s.bytesTx ?? 0))
            if let connectedAt = s.lastConnectedAtMs {
                LabeledContent(
                    "Last connected",
                    value: formatUnixMillis(connectedAt)
                )
            }
            if let evtAt = s.lastEventAtMs {
                LabeledContent(
                    "Last event",
                    value: formatUnixMillis(evtAt)
                )
            }
        }
    }

    private func rolesSection(_ cfg: RelayEditRow) -> some View {
        Section {
            ToggleRow(label: "Read", isOn: cfg.isRead) { on in
                let nextWrite = cfg.isWrite || !on
                kernelModel.setRelayRoles(url: cfg.url, read: on, write: nextWrite)
            }
            ToggleRow(label: "Write", isOn: cfg.isWrite) { on in
                let nextRead = cfg.isRead || !on
                kernelModel.setRelayRoles(url: cfg.url, read: nextRead, write: on)
            }
        } header: {
            Text("Roles")
        } footer: {
            Text("Changing Read or Write republishes your kind:10002 relay list.")
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
        }
    }

    // MARK: - State pieces

    private var stateColor: Color {
        switch status?.connection.lowercased() {
        case "connected": return .green
        case "connecting", "reconnecting": return .yellow
        case "disconnected", "terminated", "banned", "failed": return .red
        default: return .gray
        }
    }

    private var stateLabel: String {
        guard let raw = status?.connection else { return "Unknown" }
        return raw.prefix(1).uppercased() + raw.dropFirst().lowercased()
    }

    // MARK: - Formatting

    private func formatBytes(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .binary)
    }

    private func formatUnixMillis(_ ms: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(ms) / 1000.0)
        let f = DateFormatter()
        f.dateStyle = .short
        f.timeStyle = .short
        return f.string(from: date)
    }
}

/// Toggle that commits only on user-driven changes, so re-renders from the
/// kernel snapshot don't fire a fresh write back through the FFI.
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
            localValue = newValue
            didInit = true
        }
    }
}
