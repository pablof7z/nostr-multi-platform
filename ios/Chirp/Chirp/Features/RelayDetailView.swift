import SwiftUI

struct RelayDetailView: View {
    let relay: RelayStatus
    let wireSubscriptions: [WireSubscriptionStatus]
    let logicalInterests: [LogicalInterestStatus]

    var body: some View {
        ScrollView {
            VStack(spacing: ChirpSpace.xl) {
                statusSection
                if !wireSubscriptions.isEmpty {
                    wireSubsSection
                }
                if !logicalInterests.isEmpty {
                    logicalInterestsSection
                }
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.vertical, ChirpSpace.xl)
        }
        .background(Color(.systemBackground))
        .navigationTitle(shortURL(relay.relayUrl))
        .navigationBarTitleDisplayMode(.inline)
    }

    // ── Connection status ─────────────────────────────────────────────────

    private var statusSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Status")
            GlassCard {
                VStack(spacing: 0) {
                    RelayDetailRow(label: "URL") {
                        Text(relay.relayUrl)
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textSecondary)
                            .lineLimit(2)
                            .multilineTextAlignment(.trailing)
                    }
                    RelayDetailDivider()
                    RelayDetailRow(label: "Role") {
                        Text(relay.role.capitalized)
                            .font(ChirpFont.callout.weight(.semibold))
                            .foregroundStyle(roleColor)
                    }
                    RelayDetailDivider()
                    RelayDetailRow(label: "Connection") {
                        HStack(spacing: ChirpSpace.xs) {
                            Circle()
                                .fill(connectionColor)
                                .frame(width: 8, height: 8)
                                .shadow(color: connectionColor.opacity(0.6), radius: 3)
                            Text(relay.connection.capitalized)
                                .font(ChirpFont.callout.weight(.medium))
                                .foregroundStyle(connectionColor)
                        }
                    }
                    RelayDetailDivider()
                    RelayDetailRow(label: "Auth") {
                        Text(relay.auth)
                            .font(ChirpFont.mono)
                            .foregroundStyle(authColor)
                    }
                    RelayDetailDivider()
                    RelayDetailRow(label: "Active Subs") {
                        Text("\(relay.activeWireSubscriptions)")
                            .font(ChirpFont.mono)
                            .foregroundStyle(relay.activeWireSubscriptions > 10 ? ChirpColor.like : ChirpColor.textPrimary)
                            .monospacedDigit()
                    }
                    RelayDetailDivider()
                    RelayDetailRow(label: "Reconnects") {
                        Text("\(relay.reconnectCount)")
                            .font(ChirpFont.mono)
                            .foregroundStyle(relay.reconnectCount > 0 ? ChirpColor.zap : ChirpColor.textTertiary)
                            .monospacedDigit()
                    }
                    if let rx = relay.bytesRx {
                        RelayDetailDivider()
                        RelayDetailRow(label: "Bytes Rx") {
                            Text(formatBytes(rx))
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textSecondary)
                        }
                    }
                    if let tx = relay.bytesTx {
                        RelayDetailDivider()
                        RelayDetailRow(label: "Bytes Tx") {
                            Text(formatBytes(tx))
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textSecondary)
                        }
                    }
                    if let ms = relay.lastConnectedAtMs {
                        RelayDetailDivider()
                        RelayDetailRow(label: "Last Connected") {
                            Text(msToRelative(ms))
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textSecondary)
                        }
                    }
                    if let ms = relay.lastEventAtMs {
                        RelayDetailDivider()
                        RelayDetailRow(label: "Last Event") {
                            Text(msToRelative(ms))
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textSecondary)
                        }
                    }
                    if let notice = relay.lastNotice {
                        RelayDetailDivider()
                        RelayDetailRow(label: "Last Notice") {
                            Text(notice)
                                .font(ChirpFont.caption)
                                .foregroundStyle(ChirpColor.zap)
                                .multilineTextAlignment(.trailing)
                        }
                    }
                    if let error = relay.lastError {
                        RelayDetailDivider()
                        RelayDetailRow(label: "Last Error") {
                            Text(error)
                                .font(ChirpFont.caption)
                                .foregroundStyle(ChirpColor.like)
                                .multilineTextAlignment(.trailing)
                        }
                    }
                }
            }
        }
    }

    // ── Wire subscriptions ────────────────────────────────────────────────

    private var wireSubsSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Wire Subscriptions (\(wireSubscriptions.count))")
            GlassCard {
                VStack(spacing: 0) {
                    ForEach(Array(wireSubscriptions.enumerated()), id: \.element.id) { index, sub in
                        WireSubRow(sub: sub)
                        if index < wireSubscriptions.count - 1 {
                            Divider().background(ChirpColor.hairline)
                        }
                    }
                }
            }
        }
    }

    // ── Logical interests ─────────────────────────────────────────────────

    private var logicalInterestsSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Logical Interests (\(logicalInterests.count))")
            GlassCard {
                VStack(spacing: 0) {
                    ForEach(Array(logicalInterests.enumerated()), id: \.element.id) { index, interest in
                        LogicalInterestRow(interest: interest)
                        if index < logicalInterests.count - 1 {
                            Divider().background(ChirpColor.hairline)
                        }
                    }
                }
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private var connectionColor: Color {
        let s = relay.connection.lowercased()
        if s == "connected" { return ChirpColor.positive }
        if s.contains("connect") { return ChirpColor.zap }
        return ChirpColor.like
    }

    private var authColor: Color {
        let s = relay.auth.lowercased()
        if s == "ok" || s == "authenticated" { return ChirpColor.positive }
        if s == "pending" { return ChirpColor.zap }
        return ChirpColor.textTertiary
    }

    private var roleColor: Color {
        switch relay.role {
        case "read": return Color.blue
        case "write": return ChirpColor.positive
        default: return ChirpColor.accent
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

// ── Wire subscription row ──────────────────────────────────────────────────

private struct WireSubRow: View {
    let sub: WireSubscriptionStatus

    var body: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.xs) {
            HStack(alignment: .center, spacing: ChirpSpace.s) {
                Text(shortID(sub.wireId))
                    .font(ChirpFont.mono)
                    .foregroundStyle(ChirpColor.textPrimary)
                Spacer(minLength: 0)
                stateChip
            }
            Text(sub.filterSummary)
                .font(ChirpFont.caption)
                .foregroundStyle(ChirpColor.textSecondary)
                .lineLimit(2)
            HStack(spacing: ChirpSpace.s) {
                if sub.logicalConsumerCount > 0 {
                    Label("\(sub.logicalConsumerCount) consumer\(sub.logicalConsumerCount == 1 ? "" : "s")", systemImage: "person.2")
                        .font(ChirpFont.caption)
                        .foregroundStyle(ChirpColor.textTertiary)
                }
                if sub.eoseAtMs != nil {
                    Label("EOSE", systemImage: "checkmark.circle")
                        .font(ChirpFont.caption)
                        .foregroundStyle(ChirpColor.positive)
                }
                if let reason = sub.closeReason {
                    Label(reason, systemImage: "xmark.circle")
                        .font(ChirpFont.caption)
                        .foregroundStyle(ChirpColor.like)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, ChirpSpace.s)
    }

    private var stateChip: some View {
        let color = stateColor(sub.state)
        return Text(sub.state.capitalized)
            .font(.system(.caption2, design: .rounded).weight(.semibold))
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
        case "open", "active": return ChirpColor.positive
        case "pending", "warming": return ChirpColor.zap
        case "closed", "done": return ChirpColor.textTertiary
        default: return ChirpColor.textTertiary
        }
    }
}

// ── Logical interest row ───────────────────────────────────────────────────

private struct LogicalInterestRow: View {
    let interest: LogicalInterestStatus

    var body: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.xs) {
            HStack(alignment: .center, spacing: ChirpSpace.s) {
                Text(interest.key)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 0)
                stateChip
            }
            HStack(spacing: ChirpSpace.m) {
                Label("×\(interest.refcount)", systemImage: "link")
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)
                Text(interest.cacheCoverage)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)
            }
        }
        .padding(.vertical, ChirpSpace.s)
    }

    private var stateChip: some View {
        let color = stateColor(interest.state)
        return Text(interest.state.capitalized)
            .font(.system(.caption2, design: .rounded).weight(.semibold))
            .foregroundStyle(color)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(color.opacity(0.15), in: Capsule())
    }

    private func stateColor(_ s: String) -> Color {
        switch s.lowercased() {
        case "satisfied", "active": return ChirpColor.positive
        case "warming", "pending": return ChirpColor.zap
        default: return ChirpColor.textTertiary
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
                .font(ChirpFont.caption.weight(.medium))
                .foregroundStyle(ChirpColor.textTertiary)
                .frame(width: 120, alignment: .leading)
            Spacer(minLength: ChirpSpace.s)
            value
        }
        .padding(.vertical, ChirpSpace.s)
    }
}

private struct RelayDetailDivider: View {
    var body: some View {
        Divider().background(ChirpColor.hairline)
    }
}
