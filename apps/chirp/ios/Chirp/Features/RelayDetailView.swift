import SwiftUI

// Relay detail screen. THIN SHELL — every aggregate (active / EOSE'd /
// total sub counts, total events_rx), every display string (relative-time
// labels, role / connection / auth labels, byte counters) is pre-computed by
// the Rust `relay_diagnostics` projection. The view renders fields directly.
//
// NO `.filter` / `.sorted` / `.reduce`, NO `Date(timeIntervalSince1970:)`,
// NO `switch` on protocol semantics for business logic (aim.md §4.5 / §6
// anti-pattern #1). Color is shell-owned: `DiagnosticsTone` derives a hue
// token from the raw role / connection / auth / state tokens and
// `DiagnosticsColor.color(forTone:)` paints it (#1768 — rendering, not policy).

struct RelayDetailView: View {
    let row: RelayDiagnosticsRow
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                statusSection
                RelayReasonsSection(row: row)
                if let info = row.info {
                    infoSection(info)
                }
                if !row.notices.isEmpty {
                    noticesSection
                }
                subsOverviewSection
                if !row.wireSubs.isEmpty {
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
                        .foregroundStyle(DiagnosticsColor.color(forTone: DiagnosticsTone.role(row.role)))
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Connection") {
                    HStack(spacing: 4) {
                        Circle()
                            .fill(DiagnosticsColor.color(forTone: DiagnosticsTone.connection(row.connection)))
                            .frame(width: 8, height: 8)
                        Text(row.connectionLabel)
                            .font(.callout.weight(.medium))
                            .foregroundStyle(DiagnosticsColor.color(forTone: DiagnosticsTone.connection(row.connection)))
                    }
                }
                RelayDetailDivider()
                RelayDetailRow(label: "Auth") {
                    Text(row.authLabel)
                        .font(.body.monospaced())
                        .foregroundStyle(DiagnosticsColor.color(forTone: DiagnosticsTone.auth(row.auth)))
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
                if row.lastConnectedMs > 0 {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Last Connected") {
                        Text((row.lastConnectedMs / 1000).relativeTimeFromUnixSeconds)
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
                if row.lastEventMs > 0 {
                    RelayDetailDivider()
                    RelayDetailRow(label: "Last Event") {
                        Text((row.lastEventMs / 1000).relativeTimeFromUnixSeconds)
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

    // NOTICE log — bounded list of the last ≤32 NOTICEs, newest first. Each
    // entry carries a wall-clock Unix-ms timestamp rendered as "Xs ago" by
    // `relativeTimeFromUnixSeconds` (aim.md §62 — no Swift-side date formatting).
    // `noticeCount` shows the total (may exceed 32); the ring shows the retained
    // tail. Thin-shell rule: no Swift-side sorting or filtering — order from Rust.
    private var noticesSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Notices")
                    .font(.headline)
                    .foregroundStyle(.primary)
                Spacer()
                if row.noticeCount > UInt64(row.notices.count) {
                    Text("\(row.noticeCount) total")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            VStack(spacing: 0) {
                ForEach(Array(row.notices.enumerated()), id: \.offset) { index, notice in
                    if index > 0 { RelayDetailDivider() }
                    VStack(alignment: .leading, spacing: 2) {
                        Text((notice.atMs / 1000).relativeTimeFromUnixSeconds)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                        Text(notice.text)
                            .font(.caption)
                            .foregroundStyle(ChirpColor.warning)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.vertical, 6)
                }
            }
            .padding(.horizontal, 12)
        }
    }

    // ADR-0051 — NIP-11 relay information document. Every field is a
    // pre-decoded value off `row.info` (the Rust `relay_diagnostics` projection
    // surfaces the parsed document); the view only renders values it is handed.
    // Absent fields (`nil`) are omitted — byte-faithful to the typed wire's
    // `has_*` / JSON `null` semantics. NO parsing, NO HTTP, NO NIP-11 awareness
    // in the shell (aim.md §4.5 thin-shell rule).
    private func infoSection(_ info: RelayDiagnosticsInfo) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Relay Info")
                .font(.headline)
                .foregroundStyle(.primary)
            let rows = infoRows(info)
            VStack(spacing: 0) {
                ForEach(Array(rows.enumerated()), id: \.offset) { index, entry in
                    if index > 0 {
                        RelayDetailDivider()
                    }
                    RelayDetailRow(label: entry.label) { entry.value }
                }
            }
            .padding(.horizontal, 12)
        }
    }

    /// Present NIP-11 fields as label/value rows, in document order. A field
    /// absent on `row.info` (`nil`) is skipped; the booleans render their carried
    /// tri-state value (omitted when not advertised). Pure presentation — the
    /// values themselves are decided by the Rust projection.
    private func infoRows(_ info: RelayDiagnosticsInfo) -> [RelayInfoEntry] {
        var rows: [RelayInfoEntry] = []
        if let name = info.name {
            rows.append(.init(label: "Name", value: AnyView(
                Text(name)
                    .font(.body)
                    .foregroundStyle(.primary)
                    .multilineTextAlignment(.trailing)
            )))
        }
        if let description = info.description {
            rows.append(.init(label: "Description", value: AnyView(
                Text(description)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.trailing)
            )))
        }
        if let software = info.software {
            rows.append(.init(label: "Software", value: AnyView(
                Text(software)
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.trailing)
            )))
        }
        if let version = info.version {
            rows.append(.init(label: "Version", value: AnyView(
                Text(version)
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
            )))
        }
        if let contact = info.contact {
            rows.append(.init(label: "Contact", value: AnyView(
                Text(contact)
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.trailing)
            )))
        }
        if let pubkey = info.pubkey {
            rows.append(.init(label: "Pubkey", value: AnyView(
                Text(pubkey)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            )))
        }
        if !info.supportedNips.isEmpty {
            rows.append(.init(label: "Supported NIPs", value: AnyView(
                Text(info.supportedNips.map(String.init).joined(separator: ", "))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.trailing)
            )))
        }
        if let payment = info.paymentRequired {
            rows.append(.init(label: "Payment Required", value: AnyView(capabilityValue(payment))))
        }
        if let auth = info.authRequired {
            rows.append(.init(label: "Auth Required", value: AnyView(capabilityValue(auth))))
        }
        if let restricted = info.restrictedWrites {
            rows.append(.init(label: "Restricted Writes", value: AnyView(capabilityValue(restricted))))
        }
        return rows
    }

    @ViewBuilder
    private func capabilityValue(_ value: Bool) -> some View {
        Text(value ? "Yes" : "No")
            .font(.body.monospaced())
            .foregroundStyle(value ? ChirpColor.warning : ChirpColor.textSecondary)
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
                    color: row.activeSubCount == 0 ? ChirpColor.textSecondary : ChirpColor.accent
                )
            }
            HStack(spacing: 12) {
                RelayMetricTile(
                    label: "Events Rx",
                    value: row.totalEventsDisplay,
                    icon: "arrow.down.circle",
                    color: ChirpColor.accent
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

/// A single label/value row in the NIP-11 relay-info section. The `value` is a
/// pre-built passive view (no policy) — the section renders these in order with
/// dividers between them.
private struct RelayInfoEntry {
    let label: String
    let value: AnyView
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
                    .foregroundStyle(DiagnosticsColor.color(forTone: DiagnosticsTone.wireSubState(sub.state)))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(
                        DiagnosticsColor.color(forTone: DiagnosticsTone.wireSubState(sub.state)).opacity(0.15),
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
