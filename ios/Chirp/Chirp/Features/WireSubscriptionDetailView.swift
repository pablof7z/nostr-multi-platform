import SwiftUI

// Wire-subscription detail screen. THIN SHELL — every display string
// (relative-time labels, state label + tone, consumer-count prose,
// compact events_rx) is pre-formatted by the Rust `relay_diagnostics`
// projection (`RelayDiagnosticsWireSub`). The view renders fields
// directly.
//
// NO `Date(timeIntervalSince1970:)`, NO `switch` on protocol semantics
// (aim.md §4.5 / §6 anti-pattern #1 / §"Where do views live?").

struct WireSubscriptionDetailView: View {
    let sub: RelayDiagnosticsWireSub

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

    private var statsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Stats")
                .font(.headline)
                .foregroundStyle(.primary)
            HStack(spacing: 12) {
                WireMetricTile(
                    label: "Events Rx",
                    value: sub.eventsRxDisplay ?? "—",
                    icon: "arrow.down.circle",
                    color: .green
                )
                WireMetricTile(
                    label: "Consumers",
                    value: sub.consumerCountLabel.isEmpty ? "0" : sub.consumerCountLabel,
                    icon: "person.2",
                    color: .accentColor
                )
                WireMetricTile(
                    label: "EOSE",
                    value: sub.eoseObserved ? "Done" : "Pending",
                    icon: sub.eoseObserved ? "checkmark.circle.fill" : "clock",
                    color: sub.eoseObserved ? .green : .secondary
                )
            }
        }
    }

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
                    Text(sub.stateLabel)
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(DiagnosticsColor.color(forTone: sub.stateTone))
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
        }
    }

    private var timingSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Timing")
                .font(.headline)
                .foregroundStyle(.primary)
            VStack(spacing: 0) {
                SubDetailRow(label: "Opened") {
                    Text(sub.openedDisplay)
                        .font(.body.monospaced())
                        .foregroundStyle(.secondary)
                }
                if let last = sub.lastEventDisplay {
                    SubDetailDivider()
                    SubDetailRow(label: "Last Event") {
                        Text(last)
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if let eose = sub.eoseDisplay {
                    SubDetailDivider()
                    SubDetailRow(label: "EOSE At") {
                        Text(eose)
                            .font(.body.monospaced())
                            .foregroundStyle(.green)
                    }
                }
            }
            .padding(.horizontal, 12)
        }
    }

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
                .textSelection(.enabled)
        }
    }
}

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
