import SwiftUI

struct WireSubscriptionDetailView: View {
    let sub: WireSubscriptionStatus

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                statsSection
                detailsSection
                timingSection
                if let reason = sub.closeReason {
                    closeReasonSection(reason)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 24)
        }
        .chirpScreenBackground()
        .navigationTitle("Subscription")
        .navigationBarTitleDisplayMode(.inline)
    }

    // ── Stats tiles ───────────────────────────────────────────────────────

    private var statsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Stats")
                .font(.headline)
                .foregroundStyle(.primary)
            HStack(spacing: 12) {
                WireMetricTile(
                    label: "Events Rx",
                    value: sub.eventsRx.map { $0.formatted(.number.notation(.compactName)) } ?? "—",
                    icon: "arrow.down.circle",
                    color: .green
                )
                WireMetricTile(
                    label: "Consumers",
                    value: "\(sub.logicalConsumerCount)",
                    icon: "person.2",
                    color: .accentColor
                )
                WireMetricTile(
                    label: "EOSE",
                    value: sub.eoseAtMs != nil ? "Done" : "Pending",
                    icon: sub.eoseAtMs != nil ? "checkmark.circle.fill" : "clock",
                    color: sub.eoseAtMs != nil ? .green : .secondary
                )
            }
        }
    }

    // ── Subscription details ──────────────────────────────────────────────

    private var detailsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Details")
                .font(.headline)
                .foregroundStyle(.primary)
            VStack(spacing: 0) {
                SubDetailRow(label: "ID") {
                    Text(sub.wireId)
                        .font(.body.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                        .multilineTextAlignment(.trailing)
                        .textSelection(.enabled)
                }
                SubDetailDivider()
                SubDetailRow(label: "State") {
                    Text(sub.state.capitalized)
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(stateColor(sub.state))
                }
                SubDetailDivider()
                SubDetailRow(label: "Relay") {
                    Text(sub.relayUrl)
                        .font(.body.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .multilineTextAlignment(.trailing)
                }
                SubDetailDivider()
                SubDetailRow(label: "Filter") {
                    Text(sub.filterSummary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.trailing)
                }
            }
            .padding(.horizontal, 12)
            .chirpGlass(cornerRadius: 12)
        }
    }

    // ── Timing ────────────────────────────────────────────────────────────

    private var timingSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Timing")
                .font(.headline)
                .foregroundStyle(.primary)
            VStack(spacing: 0) {
                SubDetailRow(label: "Opened") {
                    Text(msToRelative(sub.openedAtMs))
                        .font(.body.monospaced())
                        .foregroundStyle(.secondary)
                }
                if let ms = sub.lastEventAtMs {
                    SubDetailDivider()
                    SubDetailRow(label: "Last Event") {
                        Text(msToRelative(ms))
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if let ms = sub.eoseAtMs {
                    SubDetailDivider()
                    SubDetailRow(label: "EOSE At") {
                        Text(msToRelative(ms))
                            .font(.body.monospaced())
                            .foregroundStyle(.green)
                    }
                }
            }
            .padding(.horizontal, 12)
            .chirpGlass(cornerRadius: 12)
        }
    }

    // ── Close reason ──────────────────────────────────────────────────────

    @ViewBuilder
    private func closeReasonSection(_ reason: String) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Close Reason")
                .font(.headline)
                .foregroundStyle(.primary)
            Text(reason)
                .font(.caption)
                .foregroundStyle(.red)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 8)
                .padding(.horizontal, 12)
                .chirpGlass(cornerRadius: 12)
                .textSelection(.enabled)
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private func stateColor(_ s: String) -> Color {
        switch s.lowercased() {
        case "open", "active", "live": return .green
        case "pending", "warming", "opening", "auth_paused": return .orange
        default: return .secondary
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
        VStack(spacing: 4) {
            Image(systemName: icon)
                .font(.system(size: 18, weight: .semibold))
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
        .chirpGlass(cornerRadius: 12)
    }
}

private struct SubDetailRow<Value: View>: View {
    let label: String
    @ViewBuilder var value: Value

    var body: some View {
        HStack(alignment: .top) {
            Text(label)
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
                .frame(width: 80, alignment: .leading)
            Spacer(minLength: 8)
            value
        }
        .padding(.vertical, 8)
    }
}

private struct SubDetailDivider: View {
    var body: some View {
        Divider()
    }
}
