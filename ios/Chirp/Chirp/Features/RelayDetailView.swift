import SwiftUI

// Relay detail screen. THIN SHELL — every aggregate (active / EOSE'd /
// total sub counts, total events_rx), every display string (relative-time
// labels, role / connection / auth labels + tones, byte counters) is
// pre-computed by the Rust `relay_diagnostics` projection. The view
// renders fields directly.
//
// NO `.filter` / `.sorted` / `.reduce`, NO `Date(timeIntervalSince1970:)`,
// NO `switch` on protocol semantics (aim.md §4.5 / §6 anti-pattern #1 /
// §"Where do views live?"). The only Swift-side mapping is
// `DiagnosticsColor.color(forTone:)` — a tone string (decided by Rust) →
// a SwiftUI Color (rendering, not policy).

struct RelayDetailView: View {
    let row: RelayDiagnosticsRow

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                statusSection
                if !row.wireSubs.isEmpty {
                    subsOverviewSection
                    wireSubsSection
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 24)
        }
        .chirpScreenBackground()
        .navigationTitle(row.shortUrl)
        .navigationBarTitleDisplayMode(.inline)
    }

    private var statusSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Status")
                .font(.headline)
                .foregroundStyle(.primary)
            VStack(spacing: 0) {
                RelayDetailRow(label: "URL") {
                    Text(row.relayUrl)
                        .font(.body.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .multilineTextAlignment(.trailing)
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Role") {
                    Text(row.roleLabel)
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(DiagnosticsColor.color(forTone: row.roleTone))
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Connection") {
                    HStack(spacing: 4) {
                        Circle()
                            .fill(DiagnosticsColor.color(forTone: row.connectionTone))
                            .frame(width: 8, height: 8)
                        Text(row.connectionLabel)
                            .font(.callout.weight(.medium))
                            .foregroundStyle(DiagnosticsColor.color(forTone: row.connectionTone))
                    }
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Auth") {
                    Text(row.authLabel)
                        .font(.body.monospaced())
                        .foregroundStyle(DiagnosticsColor.color(forTone: row.authTone))
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Active Subs") {
                        Text("\(row.activeSubCount)")
                            .font(.body.monospaced())
                            .foregroundStyle(row.activeSubCount > 10 ? ChirpColor.danger : ChirpColor.textPrimary)
                            .monospacedDigit()
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Reconnects") {
                        Text("\(row.reconnectCount)")
                            .font(.body.monospaced())
                            .foregroundStyle(row.reconnectCount > 0 ? ChirpColor.warning : ChirpColor.textSecondary)
                            .monospacedDigit()
                }
                if let bytesRx = row.bytesRxDisplay {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Bytes Rx") {
                        Text(bytesRx)
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if let bytesTx = row.bytesTxDisplay {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Bytes Tx") {
                        Text(bytesTx)
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if let connected = row.lastConnectedDisplay {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Last Connected") {
                        Text(connected)
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if let lastEvent = row.lastEventDisplay {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Last Event") {
                        Text(lastEvent)
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if let notice = row.lastNotice {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Last Notice") {
                        Text(notice)
                            .font(.caption)
                            .foregroundStyle(ChirpColor.warning)
                            .multilineTextAlignment(.trailing)
                    }
                }
                if let error = row.lastError {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Last Error") {
                        Text(error)
                            .font(.caption)
                            .foregroundStyle(ChirpColor.danger)
                            .multilineTextAlignment(.trailing)
                    }
                }
            }
            .padding(.horizontal, 12)
        }
    }

    private var subsOverviewSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Subscription Overview")
                .font(.headline)
                .foregroundStyle(.primary)
            HStack(spacing: 12) {
                RelayMetricTile(
                    label: "Total",
                    value: "\(row.totalSubCount)",
                    icon: "dot.radiowaves.left.and.right",
                    color: ChirpColor.accent
                )
                RelayMetricTile(
                    label: "Active",
                    value: "\(row.activeSubCount)",
                    icon: "bolt.fill",
                    color: row.activeSubCount == 0 ? ChirpColor.textSecondary : ChirpColor.success
                )
            }
            HStack(spacing: 12) {
                RelayMetricTile(
                    label: "Events Rx",
                    value: row.totalEventsDisplay,
                    icon: "arrow.down.circle",
                    color: ChirpColor.success
                )
                RelayMetricTile(
                    label: "EOSE'd",
                    value: "\(row.eosedSubCount)",
                    icon: "checkmark.circle",
                    color: .secondary
                )
            }
        }
    }

    private var wireSubsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Wire Subscriptions (\(row.wireSubs.count))")
                .font(.headline)
                .foregroundStyle(.primary)
            VStack(spacing: 0) {
                ForEach(Array(row.wireSubs.enumerated()), id: \.element.id) { index, sub in
                    NavigationLink(destination: WireSubscriptionDetailView(sub: sub)) {
                        WireSubRow(sub: sub)
                    }
                    .buttonStyle(.plain)
                    if index < row.wireSubs.count - 1 {
                        Divider()
                    }
                }
            }
            .padding(.horizontal, 12)
        }
    }
}

private struct RelayMetricTile: View {
    let label: String
    let value: String
    let icon: String
    let color: Color

    var body: some View {
        VStack(spacing: 4) {
            Image(systemName: icon)
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(color)
            Text(value)
                .font(.headline)
                .foregroundStyle(.primary)
                .monospacedDigit()
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
    }
}

private struct WireSubRow: View {
    let sub: RelayDiagnosticsWireSub

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .center, spacing: 8) {
                Text(sub.shortWireId)
                    .font(.body.monospaced())
                    .foregroundStyle(.primary)
                Spacer(minLength: 0)
                Text(sub.stateLabel)
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(DiagnosticsColor.color(forTone: sub.stateTone))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(
                        DiagnosticsColor.color(forTone: sub.stateTone).opacity(0.15),
                        in: Capsule()
                    )
            }
            Text(sub.filterSummary)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(2)
            HStack(spacing: 8) {
                if !sub.consumerCountLabel.isEmpty {
                    Label(sub.consumerCountLabel, systemImage: "person.2")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if let rx = sub.eventsRxDisplay {
                    Label("\(rx) events", systemImage: "arrow.down.circle")
                        .font(.caption)
                        .foregroundStyle(ChirpColor.success)
                }
                if sub.eoseObserved {
                    Label("EOSE", systemImage: "checkmark.circle")
                        .font(.caption)
                        .foregroundStyle(ChirpColor.success)
                }
                if let reason = sub.closeReason {
                    Label(reason, systemImage: "xmark.circle")
                        .font(.caption)
                        .foregroundStyle(ChirpColor.danger)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, 8)
    }
}

private struct RelayDetailRow<Value: View>: View {
    let label: String
    @ViewBuilder var value: Value

    var body: some View {
        HStack(alignment: .top) {
            Text(label)
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
                .frame(width: 120, alignment: .leading)
            Spacer(minLength: 8)
            value
        }
        .padding(.vertical, 8)
    }
}

private struct RelayDetailDivider: View {
    var body: some View {
        Divider()
    }
}
