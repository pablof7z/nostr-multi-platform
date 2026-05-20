import SwiftUI

struct RelayDetailView: View {
    let relay: RelayStatus
    let wireSubscriptions: [WireSubscriptionStatus]
    let logicalInterests: [LogicalInterestStatus]

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                statusSection
                if !wireSubscriptions.isEmpty {
                    subsOverviewSection
                    wireSubsSection
                }
                if !logicalInterests.isEmpty {
                    logicalInterestsSection
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 24)
        }
        .chirpScreenBackground()
        .navigationTitle(shortURL(relay.relayUrl))
        .navigationBarTitleDisplayMode(.inline)
    }

    // ── Connection status ─────────────────────────────────────────────────

    private var statusSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Status")
                .font(.headline)
                .foregroundStyle(.primary)
            VStack(spacing: 0) {
                RelayDetailRow(label: "URL") {
                    Text(relay.relayUrl)
                        .font(.body.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .multilineTextAlignment(.trailing)
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Role") {
                    Text(relay.role.capitalized)
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(roleColor)
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Connection") {
                    HStack(spacing: 4) {
                        Circle()
                            .fill(connectionColor)
                            .frame(width: 8, height: 8)
                        Text(relay.connection.capitalized)
                            .font(.callout.weight(.medium))
                            .foregroundStyle(connectionColor)
                    }
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Auth") {
                    Text(relay.auth)
                        .font(.body.monospaced())
                        .foregroundStyle(authColor)
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Active Subs") {
                    Text("\(relay.activeWireSubscriptions)")
                        .font(.body.monospaced())
                        .foregroundStyle(relay.activeWireSubscriptions > 10 ? .red : .primary)
                        .monospacedDigit()
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Reconnects") {
                    Text("\(relay.reconnectCount)")
                        .font(.body.monospaced())
                        .foregroundStyle(relay.reconnectCount > 0 ? .orange : .secondary)
                        .monospacedDigit()
                }
                if let rx = relay.bytesRx, rx > 0 {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Bytes Rx") {
                        Text(formatBytes(rx))
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if let tx = relay.bytesTx, tx > 0 {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Bytes Tx") {
                        Text(formatBytes(tx))
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if let ms = relay.lastConnectedAtMs {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Last Connected") {
                        Text(msToRelative(ms))
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if let ms = relay.lastEventAtMs {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Last Event") {
                        Text(msToRelative(ms))
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if let notice = relay.lastNotice {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Last Notice") {
                        Text(notice)
                            .font(.caption)
                            .foregroundStyle(.orange)
                            .multilineTextAlignment(.trailing)
                    }
                }
                if let error = relay.lastError {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Last Error") {
                        Text(error)
                            .font(.caption)
                            .foregroundStyle(.red)
                            .multilineTextAlignment(.trailing)
                    }
                }
            }
            .padding(.horizontal, 12)
        }
    }

    // ── Subscription overview tiles ───────────────────────────────────────

    private var subsOverviewSection: some View {
        let activeSubs = wireSubscriptions.filter { ["open", "live", "active"].contains($0.state) }
        let eosedSubs = wireSubscriptions.filter { $0.eoseAtMs != nil }
        let totalEvents = wireSubscriptions.compactMap(\.eventsRx).reduce(0, +)
        return VStack(alignment: .leading, spacing: 12) {
            Text("Subscription Overview")
                .font(.headline)
                .foregroundStyle(.primary)
            HStack(spacing: 12) {
                RelayMetricTile(
                    label: "Total",
                    value: "\(wireSubscriptions.count)",
                    icon: "dot.radiowaves.left.and.right",
                    color: .accentColor
                )
                RelayMetricTile(
                    label: "Active",
                    value: "\(activeSubs.count)",
                    icon: "bolt.fill",
                    color: activeSubs.isEmpty ? .secondary : .green
                )
            }
            HStack(spacing: 12) {
                RelayMetricTile(
                    label: "Events Rx",
                    value: totalEvents.formatted(.number.notation(.compactName)),
                    icon: "arrow.down.circle",
                    color: .green
                )
                RelayMetricTile(
                    label: "EOSE'd",
                    value: "\(eosedSubs.count)",
                    icon: "checkmark.circle",
                    color: .secondary
                )
            }
        }
    }

    // ── Wire subscriptions ────────────────────────────────────────────────

    private var wireSubsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Wire Subscriptions (\(wireSubscriptions.count))")
                .font(.headline)
                .foregroundStyle(.primary)
            VStack(spacing: 0) {
                ForEach(Array(wireSubscriptions.enumerated()), id: \.element.id) { index, sub in
                    NavigationLink(destination: WireSubscriptionDetailView(sub: sub)) {
                        WireSubRow(sub: sub)
                    }
                    .buttonStyle(.plain)
                    if index < wireSubscriptions.count - 1 {
                        Divider()
                    }
                }
            }
            .padding(.horizontal, 12)
        }
    }

    // ── Logical interests ─────────────────────────────────────────────────

    private var logicalInterestsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Logical Interests (\(logicalInterests.count))")
                .font(.headline)
                .foregroundStyle(.primary)
            VStack(spacing: 0) {
                ForEach(Array(logicalInterests.enumerated()), id: \.element.id) { index, interest in
                    LogicalInterestRow(interest: interest)
                    if index < logicalInterests.count - 1 {
                        Divider()
                    }
                }
            }
            .padding(.horizontal, 12)
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private var connectionColor: Color {
        let s = relay.connection.lowercased()
        if s == "connected" { return .green }
        if s.contains("connect") { return .orange }
        return .red
    }

    private var authColor: Color {
        let s = relay.auth.lowercased()
        if s == "ok" || s == "authenticated" { return .green }
        if s == "pending" { return .orange }
        return .secondary
    }

    private var roleColor: Color {
        switch relay.role {
        case "read": return .accentColor
        case "write": return .green
        default: return .accentColor
        }
    }

    private func shortURL(_ url: String) -> String {
        url.replacingOccurrences(of: "wss://", with: "")
            .replacingOccurrences(of: "ws://", with: "")
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    }

    private func formatBytes(_ bytes: UInt64) -> String {
        let kb = Double(bytes) / 1024
        if kb < 1024 { return String(format: "%.1f KB", kb) }
        return String(format: "%.1f MB", kb / 1024)
    }

    private func msToRelative(_ ms: UInt64) -> String {
        let date = Date(timeIntervalSince1970: Double(ms) / 1000)
        let diff = Date().timeIntervalSince(date)
        if diff < 60 { return "\(Int(diff))s ago" }
        if diff < 3600 { return "\(Int(diff / 60))m ago" }
        return "\(Int(diff / 3600))h ago"
    }
}

// ── Metric tile ───────────────────────────────────────────────────────────

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

// ── Wire subscription row ──────────────────────────────────────────────────

private struct WireSubRow: View {
    let sub: WireSubscriptionStatus

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .center, spacing: 8) {
                Text(shortID(sub.wireId))
                    .font(.body.monospaced())
                    .foregroundStyle(.primary)
                Spacer(minLength: 0)
                stateChip
            }
            Text(sub.filterSummary)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(2)
            HStack(spacing: 8) {
                if sub.logicalConsumerCount > 0 {
                    Label("\(sub.logicalConsumerCount) consumer\(sub.logicalConsumerCount == 1 ? "" : "s")", systemImage: "person.2")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if let rx = sub.eventsRx, rx > 0 {
                    Label("\(rx.formatted(.number.notation(.compactName))) events", systemImage: "arrow.down.circle")
                        .font(.caption)
                        .foregroundStyle(.green)
                }
                if sub.eoseAtMs != nil {
                    Label("EOSE", systemImage: "checkmark.circle")
                        .font(.caption)
                        .foregroundStyle(.green)
                }
                if let reason = sub.closeReason {
                    Label(reason, systemImage: "xmark.circle")
                        .font(.caption)
                        .foregroundStyle(.red)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, 8)
    }

    private var stateChip: some View {
        let color = stateColor(sub.state)
        return Text(sub.state.capitalized)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(color)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(color.opacity(0.15), in: Capsule())
    }

    private func shortID(_ id: String) -> String {
        guard id.count > 12 else { return id }
        return "\(id.prefix(8))…"
    }

    private func stateColor(_ s: String) -> Color {
        switch s.lowercased() {
        case "open", "active", "live": return .green
        case "pending", "warming", "opening", "auth_paused": return .orange
        case "closed", "done": return .secondary
        default: return .secondary
        }
    }
}

// ── Logical interest row ───────────────────────────────────────────────────

private struct LogicalInterestRow: View {
    let interest: LogicalInterestStatus

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .center, spacing: 8) {
                Text(interest.key)
                    .font(.caption)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 0)
                stateChip
            }
            HStack(spacing: 12) {
                Label("×\(interest.refcount)", systemImage: "link")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(interest.cacheCoverage)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 8)
    }

    private var stateChip: some View {
        let color = stateColor(interest.state)
        return Text(interest.state.capitalized)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(color)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(color.opacity(0.15), in: Capsule())
    }

    private func stateColor(_ s: String) -> Color {
        switch s.lowercased() {
        case "satisfied", "active": return .green
        case "warming", "pending": return .orange
        default: return .secondary
        }
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────────

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
