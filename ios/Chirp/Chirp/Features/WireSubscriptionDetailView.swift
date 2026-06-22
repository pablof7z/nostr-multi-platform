import SwiftUI

// Wire-subscription detail screen. THIN SHELL — Rust owns subscription state,
// diagnostic categorization, and projection-provided status labels. The view
// renders those fields directly without deriving protocol semantics.
//
// NO `switch` on protocol semantics (aim.md §4.5 / §"Where do views live?").

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
                    color: ChirpColor.success
                )
                WireMetricTile(
                    label: "Consumers",
                    value: sub.consumerCountLabel.isEmpty ? "0" : sub.consumerCountLabel,
                    icon: "person.2",
                    color: ChirpColor.accent
                )
                WireMetricTile(
                    label: "EOSE",
                    value: sub.eoseObserved ? "Done" : "Pending",
                    icon: sub.eoseObserved ? "checkmark.circle.fill" : "clock",
                    color: sub.eoseObserved ? ChirpColor.success : ChirpColor.textSecondary
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
                        .foregroundStyle(DiagnosticsColor.color(forTone: DiagnosticsTone.wireSubState(sub.state)))
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
                    Text((sub.openedMs / 1000).relativeTimeFromUnixSeconds)
                        .font(.body.monospaced())
                        .foregroundStyle(.secondary)
                }
                if sub.lastEventMs > 0 {
                    SubDetailDivider()
                    SubDetailRow(label: "Last Event") {
                        Text((sub.lastEventMs / 1000).relativeTimeFromUnixSeconds)
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if sub.eoseMs > 0 {
                    SubDetailDivider()
                    SubDetailRow(label: "EOSE At") {
                        Text((sub.eoseMs / 1000).relativeTimeFromUnixSeconds)
                            .font(.body.monospaced())
                            .foregroundStyle(ChirpColor.success)
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
                .foregroundStyle(ChirpColor.danger)
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

// MARK: - Diagnostics color policy
//
// `DiagnosticsColor` + `DiagnosticsTone` were extracted out of
// `DiagnosticsView.swift` to keep that file under the 500-LOC hard-cap gate
// (AGENTS.md) once #1768 moved tone derivation shell-side. They are shared by
// every relay-diagnostics view (DiagnosticsView / RelayDetailView /
// RelayReasonsSection / this file). Co-located here (a registered SwiftUI
// source) rather than a new file to avoid xcodegen pbxproj churn.

/// Single Swift-side helper: map a SEMANTIC tone string to a SwiftUI Color.
/// This is rendering, not policy — the shell decides how to paint each class.
/// The tone itself is now derived shell-side from raw protocol tokens by
/// `DiagnosticsTone` (#1768 — core emits raw tokens only).
enum DiagnosticsColor {
    static func color(forTone tone: String) -> Color {
        switch tone {
        case "ok": return ChirpColor.success
        case "warn": return ChirpColor.warning
        case "error": return ChirpColor.danger
        case "write": return ChirpColor.success
        case "accent": return ChirpColor.accent
        case "primary": return ChirpColor.accent
        case "muted", "secondary": return ChirpColor.textSecondary
        default: return ChirpColor.textSecondary
        }
    }
}

/// Shell-side tone policy (#1768): derive a semantic hue token from the RAW
/// protocol tokens the `relay_diagnostics` projection now emits. The kernel
/// emits only raw `role` / `connection` / `auth` / `state` / reason `kind`
/// strings; deciding which hue class each belongs to is the app's job. Feeds
/// `DiagnosticsColor.color(forTone:)`. Ported verbatim from the former kernel
/// `relay_diagnostics/format.rs` + `reasons.rs` selectors.
enum DiagnosticsTone {
    /// Relay role → tone.
    static func role(_ role: String) -> String {
        role == "write" ? "write" : "accent"
    }

    /// Relay connection → tone.
    static func connection(_ connection: String) -> String {
        let lower = connection.lowercased()
        if lower == "connected" {
            return "ok"
        } else if lower.hasPrefix("disconnect") || lower == "failed" {
            return "error"
        } else if lower.contains("connect") {
            return "warn"
        } else if lower == "unknown" || lower == "idle" || lower == "—" || lower == "blocked" {
            return "muted"
        } else {
            return "error"
        }
    }

    /// Relay auth → tone.
    static func auth(_ auth: String) -> String {
        let lower = auth.lowercased()
        if lower == "ok" || lower == "authenticated" {
            return "ok"
        } else if lower == "pending" {
            return "warn"
        } else {
            return "muted"
        }
    }

    /// Wire-subscription state → tone.
    static func wireSubState(_ state: String) -> String {
        switch state.lowercased() {
        case "open", "active", "live": return "ok"
        case "pending", "warming", "opening", "auth_paused": return "warn"
        default: return "muted"
        }
    }

    /// Logical-interest state → tone.
    static func interestState(_ state: String) -> String {
        switch state {
        case "active", "warming", "tailing", "complete": return "ok"
        case "idle": return "muted"
        default: return "warn"
        }
    }

    /// Connection-reason `kind` → tone.
    static func reason(_ kind: String) -> String {
        switch kind {
        case "blocked": return "muted"
        case "nip65": return "accent"
        case "hint": return "warn"
        default: return "ok"
        }
    }
}
