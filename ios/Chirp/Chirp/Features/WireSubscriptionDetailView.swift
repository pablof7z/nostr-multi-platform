import SwiftUI

struct WireSubscriptionDetailView: View {
    let sub: WireSubscriptionStatus

    var body: some View {
        ScrollView {
            VStack(spacing: ChirpSpace.xl) {
                statsSection
                detailsSection
                timingSection
                if let reason = sub.closeReason {
                    closeReasonSection(reason)
                }
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.vertical, ChirpSpace.xl)
        }
        .background(Color(.systemBackground))
        .navigationTitle("Subscription")
        .navigationBarTitleDisplayMode(.inline)
    }

    // ── Stats tiles ───────────────────────────────────────────────────────

    private var statsSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Stats")
            HStack(spacing: ChirpSpace.m) {
                WireMetricTile(
                    label: "Events Rx",
                    value: sub.eventsRx.map { $0.formatted(.number.notation(.compactName)) } ?? "—",
                    icon: "arrow.down.circle",
                    color: ChirpColor.positive
                )
                WireMetricTile(
                    label: "Consumers",
                    value: "\(sub.logicalConsumerCount)",
                    icon: "person.2",
                    color: ChirpColor.accent
                )
                WireMetricTile(
                    label: "EOSE",
                    value: sub.eoseAtMs != nil ? "Done" : "Pending",
                    icon: sub.eoseAtMs != nil ? "checkmark.circle.fill" : "clock",
                    color: sub.eoseAtMs != nil ? ChirpColor.positive : ChirpColor.textTertiary
                )
            }
        }
    }

    // ── Subscription details ──────────────────────────────────────────────

    private var detailsSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Details")
            GlassCard {
                VStack(spacing: 0) {
                    SubDetailRow(label: "ID") {
                        Text(sub.wireId)
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textSecondary)
                            .lineLimit(3)
                            .multilineTextAlignment(.trailing)
                            .textSelection(.enabled)
                    }
                    SubDetailDivider()
                    SubDetailRow(label: "State") {
                        Text(sub.state.capitalized)
                            .font(ChirpFont.callout.weight(.semibold))
                            .foregroundStyle(stateColor(sub.state))
                    }
                    SubDetailDivider()
                    SubDetailRow(label: "Relay") {
                        Text(sub.relayUrl)
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textSecondary)
                            .lineLimit(2)
                            .multilineTextAlignment(.trailing)
                    }
                    SubDetailDivider()
                    SubDetailRow(label: "Filter") {
                        Text(sub.filterSummary)
                            .font(ChirpFont.caption)
                            .foregroundStyle(ChirpColor.textSecondary)
                            .multilineTextAlignment(.trailing)
                    }
                }
            }
        }
    }

    // ── Timing ────────────────────────────────────────────────────────────

    private var timingSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Timing")
            GlassCard {
                VStack(spacing: 0) {
                    SubDetailRow(label: "Opened") {
                        Text(msToRelative(sub.openedAtMs))
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textSecondary)
                    }
                    if let ms = sub.lastEventAtMs {
                        SubDetailDivider()
                        SubDetailRow(label: "Last Event") {
                            Text(msToRelative(ms))
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textSecondary)
                        }
                    }
                    if let ms = sub.eoseAtMs {
                        SubDetailDivider()
                        SubDetailRow(label: "EOSE At") {
                            Text(msToRelative(ms))
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.positive)
                        }
                    }
                }
            }
        }
    }

    // ── Close reason ──────────────────────────────────────────────────────

    @ViewBuilder
    private func closeReasonSection(_ reason: String) -> some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Close Reason")
            GlassCard {
                Text(reason)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.like)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, ChirpSpace.xs)
                    .textSelection(.enabled)
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private func stateColor(_ s: String) -> Color {
        switch s.lowercased() {
        case "open", "active", "live": return ChirpColor.positive
        case "pending", "warming", "opening", "auth_paused": return ChirpColor.zap
        default: return ChirpColor.textTertiary
        }
    }

    private func msToRelative(_ ms: UInt64) -> String {
        guard ms > 0 else { return "—" }
        let date = Date(timeIntervalSince1970: Double(ms) / 1000)
        let diff = Date().timeIntervalSince(date)
        if diff < 60 { return "\(Int(diff))s ago" }
        if diff < 3600 { return "\(Int(diff / 60))m ago" }
        return "\(Int(diff / 3600))h ago"
    }
}

// ── Sub-components ────────────────────────────────────────────────────────

private struct WireMetricTile: View {
    let label: String
    let value: String
    let icon: String
    let color: Color

    var body: some View {
        GlassCard {
            VStack(spacing: ChirpSpace.xs) {
                Image(systemName: icon)
                    .font(.system(size: 18, weight: .semibold))
                    .foregroundStyle(color)
                Text(value)
                    .font(ChirpFont.headline)
                    .foregroundStyle(ChirpColor.textPrimary)
                    .monospacedDigit()
                Text(label)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, ChirpSpace.xs)
        }
    }
}

private struct SubDetailRow<Value: View>: View {
    let label: String
    @ViewBuilder var value: Value

    var body: some View {
        HStack(alignment: .top) {
            Text(label)
                .font(ChirpFont.caption.weight(.medium))
                .foregroundStyle(ChirpColor.textTertiary)
                .frame(width: 80, alignment: .leading)
            Spacer(minLength: ChirpSpace.s)
            value
        }
        .padding(.vertical, ChirpSpace.s)
    }
}

private struct SubDetailDivider: View {
    var body: some View {
        Divider().background(ChirpColor.hairline)
    }
}
