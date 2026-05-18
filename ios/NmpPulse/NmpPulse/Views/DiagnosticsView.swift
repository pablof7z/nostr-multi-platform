import SwiftUI

/// D6 + D8 observability surface. Mirrors what the kernel emits on each
/// snapshot. No platform-side derivation — every field is verbatim from
/// the JSON the actor pushes.
struct DiagnosticsView: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        List {
            Section("Kernel snapshot") {
                LabeledContent("rev", value: "\(model.rev)")
                LabeledContent("snapshots received", value: "\(model.snapshotCount)")
                LabeledContent("running", value: model.isRunning ? "yes" : "no")
                LabeledContent("last snapshot", value: lastSnapshotRelative)
            }

            if let metrics = model.metrics {
                Section("Metrics") {
                    LabeledContent("stored events", value: "\(metrics.storedEvents)")
                    LabeledContent("visible items", value: "\(metrics.visibleItems)")
                    LabeledContent("events RX", value: "\(metrics.eventsRx)")
                    LabeledContent("update sequence", value: "\(metrics.updateSequence)")
                }
            }

            Section("Relays") {
                if model.relayStatuses.isEmpty {
                    Text("No relay status yet")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(model.relayStatuses) { relay in
                        RelayStatusRow(relay: relay)
                    }
                }
            }
        }
        .navigationTitle("Diagnostics")
    }

    private var lastSnapshotRelative: String {
        guard let t = model.lastSnapshotAt else { return "never" }
        let elapsed = Date().timeIntervalSince(t)
        if elapsed < 1 {
            return "just now"
        }
        return "\(Int(elapsed))s ago"
    }
}

private struct RelayStatusRow: View {
    let relay: RelayStatus

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(relay.relayUrl)
                .font(.body)
                .lineLimit(1)
                .truncationMode(.middle)
            HStack {
                Label(relay.connection, systemImage: connectionIcon)
                    .foregroundStyle(connectionTint)
                    .font(.caption)
                Spacer()
                Text("auth: \(relay.auth)")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                Text("\(relay.activeWireSubscriptions) subs")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
    }

    private var connectionIcon: String {
        switch relay.connection {
        case "connected": return "checkmark.circle.fill"
        case "connecting": return "ellipsis.circle"
        case "failed", "error": return "exclamationmark.triangle.fill"
        case "closed": return "xmark.circle"
        default: return "questionmark.circle"
        }
    }

    private var connectionTint: Color {
        switch relay.connection {
        case "connected": return .green
        case "connecting": return .yellow
        case "failed", "error": return .red
        default: return .secondary
        }
    }
}
