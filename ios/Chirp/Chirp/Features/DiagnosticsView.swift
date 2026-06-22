import SwiftUI

// Diagnostics screen. THIN SHELL — fields are rendered directly from the
// kernel's `relay_diagnostics` projection plus the existing scalar metrics.
// NO `.filter` / `.sorted` / `.reduce` / `.first(where:)`, NO
// `Date(timeIntervalSince1970:)`, NO `switch` on protocol semantics: the
// Rust projection owns the relay-row + wire-sub roll-ups and every
// relative-time string (aim.md §4.5 / §6 anti-pattern #1). The shell derives
// its OWN color (tone) from the raw role / connection / auth / state tokens
// via `DiagnosticsTone` (#1768 — core emits raw tokens, never semantic hue).

struct DiagnosticsView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var copiedNpub = false

    var body: some View {
        List {
            kernelSection
            perfSection
            metricsSection
            relaySection
            logicalInterestsSection
            publishQueueSection
            accountSection
            runtimeLogSection
        }
        .accessibilityIdentifier("diagnostics-list")
        .navigationTitle("Diagnostics")
        .navigationBarTitleDisplayMode(.large)
    }

    private var kernelSection: some View {
        Section("Kernel") {
            HStack {
                Text("Status")
                Spacer()
                HStack(spacing: 4) {
                    Circle()
                        .fill(model.isRunning ? ChirpColor.success : ChirpColor.danger)
                        .frame(width: 8, height: 8)
                    Text(model.isRunning ? "Running" : "Stopped")
                        .font(.callout.weight(.medium))
                        .foregroundStyle(model.isRunning ? ChirpColor.success : ChirpColor.danger)
                }
            }
            HStack {
                Text("Rev")
                Spacer()
                Text("\(model.rev)")
                    .font(.body.monospaced())
                    .monospacedDigit()
            }
            HStack {
                Text("Snapshots")
                Spacer()
                Text("\(model.snapshotCount)")
                    .font(.body.monospaced())
                    .monospacedDigit()
            }
            HStack {
                Text("Last Snapshot")
                Spacer()
                if let date = model.lastSnapshotAt {
                    Text(date, style: .relative)
                        .font(.body.monospaced())
                        .foregroundStyle(.secondary)
                } else {
                    Text("—")
                        .foregroundStyle(.secondary)
                }
            }
            HStack {
                Text("Update Seq")
                Spacer()
                if let seq = model.metrics?.updateSequence {
                    Text("\(seq)")
                        .font(.body.monospaced())
                        .monospacedDigit()
                } else {
                    Text("—")
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private var perfSection: some View {
        Section("Performance") {
            HStack {
                Text("Events Rx")
                Spacer()
                Text(model.metrics.map { "\($0.eventsRx)" } ?? "—")
                    .font(.body.monospaced())
                    .monospacedDigit()
                    .accessibilityIdentifier("metric-events-value")
            }
            HStack {
                Text("Visible")
                Spacer()
                Text(model.metrics.map { "\($0.visibleItems)" } ?? "—")
                    .font(.body.monospaced())
                    .monospacedDigit()
                    .accessibilityIdentifier("metric-visible-value")
            }
            HStack {
                Text("Bytes Rx")
                Spacer()
                let value = model.metrics.map { formatBytes(Int64($0.bytesRx)) } ?? "—"
                Text(value)
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("metric-rx-value")
            }
            HStack {
                Text("First Event")
                Spacer()
                let value = model.metrics?.firstEventMs.map { "\($0) ms" } ?? "-"
                Text(value)
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("metric-first-ms-value")
            }
            HStack {
                Text("Max Apply")
                Spacer()
                Text("\(model.appMetrics.maxApplyMicros) us")
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("metric-apply-us-value")
            }
            HStack {
                Text("Decode")
                Spacer()
                Text("\(model.appMetrics.lastDecodeMicros) us")
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
            }
            HStack {
                Text("cb→screen")
                Spacer()
                Text("\(model.appMetrics.lastCallbackToAppliedMicros) us")
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
            }
            HStack {
                Text("Queue Depth")
                Spacer()
                Text(model.metrics.map { "\($0.actorQueueDepth)" } ?? "—")
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var metricsSection: some View {
        Section("Metrics") {
            if let m = model.metrics {
                HStack(spacing: 12) {
                    MetricTile(
                        label: "Stored",
                        value: m.storedEvents.formatted(.number.notation(.compactName)),
                        icon: "internaldrive"
                    )
                    MetricTile(
                        label: "Visible",
                        value: m.visibleItems.formatted(.number.notation(.compactName)),
                        icon: "eye"
                    )
                    MetricTile(
                        label: "Events Rx",
                        value: m.eventsRx.formatted(.number.notation(.compactName)),
                        icon: "arrow.down.circle"
                    )
                }
                HStack(spacing: 12) {
                    MetricTile(
                        label: "Note Events",
                        value: m.noteEvents.formatted(.number.notation(.compactName)),
                        icon: "doc.text"
                    )
                    MetricTile(
                        label: "Queue Depth",
                        value: "\(m.actorQueueDepth)",
                        icon: "tray.full"
                    )
                    MetricTile(
                        label: "Payload",
                        value: formatBytes(Int64(m.payloadBytes)),
                        icon: "doc.zipper"
                    )
                }
            } else {
                HStack {
                    ProgressView()
                    Text("Waiting for kernel…")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
            }
        }
    }

    /// Relay list. Rendered directly from `model.relayDiagnostics.relays` —
    /// the Rust projection owns merging typed lanes + outbox-only URLs,
    /// computing roll-up counters, and pre-formatting every label.
    private var relaySection: some View {
        let rows = model.relayDiagnostics.relays
        return Section("Relays (\(rows.count))") {
            if rows.isEmpty {
                Text("No relays yet")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(rows) { row in
                    NavigationLink(destination: RelayDetailView(row: row)) {
                        DiagRelayRow(row: row)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var logicalInterestsSection: some View {
        let interests = model.relayDiagnostics.interests
        if !interests.isEmpty {
            Section("Logical Interests (\(interests.count))") {
                ForEach(interests) { interest in
                    VStack(alignment: .leading, spacing: 4) {
                        Text(interest.key)
                            .font(.body.monospaced())
                            .lineLimit(1)
                        HStack(spacing: 12) {
                            Text(interest.state)
                                .font(.caption)
                                .foregroundStyle(DiagnosticsColor.color(forTone: DiagnosticsTone.interestState(interest.state)))
                            Text("ref \(interest.refcount)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            Text(interest.cacheCoverage)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    .padding(.vertical, 4)
                }
            }
        }
    }

    @ViewBuilder
    private var publishQueueSection: some View {
        if !model.publishQueue.isEmpty {
            Section("Publish Queue (\(model.publishQueue.count))") {
                ForEach(model.publishQueue) { entry in
                    DiagPublishRow(entry: entry)
                }
            }
        }
    }

    private var accountSection: some View {
        Section("Active Account") {
            if let account = model.activeAccountSummary {
                HStack {
                    Text("Display")
                    Spacer()
                    Text(account.displayName?.isEmpty == false ? account.displayName! : "—")
                        .font(.callout)
                }
                HStack {
                    Text("Signer")
                    Spacer()
                    Text(account.signerKind)
                        .font(.footnote.monospaced())
                        .foregroundStyle(.secondary)
                }
                HStack {
                    Text("Status")
                    Spacer()
                    Text(account.status)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(account.isActive ? ChirpColor.success : ChirpColor.textSecondary)
                }
                VStack(alignment: .leading, spacing: 4) {
                    Text("npub")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Button {
                        UIPasteboard.general.string = account.npub
                        let generator = UIImpactFeedbackGenerator(style: .light)
                        generator.impactOccurred()
                        copiedNpub = true
                        Task {
                            try? await Task.sleep(for: .seconds(2))
                            copiedNpub = false
                        }
                    } label: {
                        HStack(spacing: 8) {
                            Text(account.npub)
                                .font(.body.monospaced())
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                                .multilineTextAlignment(.leading)
                            Spacer(minLength: 0)
                            Image(systemName: copiedNpub ? "checkmark.circle.fill" : "doc.on.doc")
                                .foregroundStyle(copiedNpub ? ChirpColor.success : ChirpColor.accent)
                        }
                    }
                    .buttonStyle(.plain)
                }
            } else {
                HStack(spacing: 8) {
                    Image(systemName: "person.slash")
                        .foregroundStyle(.secondary)
                    Text("No active account")
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    @ViewBuilder
    private var runtimeLogSection: some View {
        if !model.logs.isEmpty {
            Section("Runtime Log (\(model.logs.count))") {
                ForEach(Array(model.logs.reversed().prefix(50).enumerated()), id: \.offset) { _, entry in
                    Text(entry)
                        .font(.body.monospaced())
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                }
            }
        }
    }

    private func formatBytes(_ bytes: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: bytes, countStyle: .binary)
    }
}

private struct MetricTile: View {
    let label: String
    let value: String
    let icon: String
    var body: some View {
        VStack(spacing: 4) {
            Image(systemName: icon)
                .font(.system(size: 18, weight: .semibold))
                .foregroundStyle(ChirpColor.accent)
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

/// One relay row. Renders fields verbatim from the projection — no
/// derivations, no protocol-keyword switches.
struct DiagRelayRow: View {
    let row: RelayDiagnosticsRow

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .center, spacing: 8) {
                Circle()
                    .fill(DiagnosticsColor.color(forTone: DiagnosticsTone.connection(row.connection)))
                    .frame(width: 8, height: 8)
                Text(row.relayUrl)
                    .font(.body.monospaced())
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 0)
                Text(row.roleLabel)
                    .font(.caption2.weight(.bold))
                    .foregroundStyle(DiagnosticsColor.color(forTone: DiagnosticsTone.role(row.role)))
            }
            HStack(spacing: 12) {
                Text(row.connectionLabel)
                    .font(.caption)
                    .foregroundStyle(DiagnosticsColor.color(forTone: DiagnosticsTone.connection(row.connection)))
                Text("Auth: \(row.authLabel)")
                    .font(.caption)
                    .foregroundStyle(DiagnosticsColor.color(forTone: DiagnosticsTone.auth(row.auth)))
                Text("\(row.activeSubCount) subs")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text("\(row.totalEventsDisplay) events")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if row.reconnectCount > 0 {
                    Text("↩ \(row.reconnectCount)")
                        .font(.caption)
                        .foregroundStyle(ChirpColor.warning)
                }
                if row.noticeCount > 0 {
                    Text("\(row.noticeCount) notice\(row.noticeCount == 1 ? "" : "s")")
                        .font(.caption)
                        .foregroundStyle(ChirpColor.warning)
                }
            }
            if let notice = row.lastNotice {
                Text(notice)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            if let error = row.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(ChirpColor.danger)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 4)
    }
}

private struct DiagPublishRow: View {
    let entry: PublishQueueEntry
    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 4) {
                    Text(shortID(entry.eventId))
                        .font(.body.monospaced())
                    Text("kind \(entry.kind)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Text("\(entry.targetRelays) relay\(entry.targetRelays == 1 ? "" : "s")")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Text(entry.status.capitalized)
                .font(.caption2.weight(.semibold))
                .foregroundStyle(statusColor(entry.status))
        }
        .padding(.vertical, 4)
    }
    private func shortID(_ id: String) -> String {
        guard id.count >= 12 else { return id }
        return "\(id.prefix(8))…"
    }
    /// Publish-queue status colors. The publish queue is a SEPARATE
    /// projection from `relay_diagnostics`; the semantic status labels come
    /// from Rust and the shell only maps them to theme tokens.
    private func statusColor(_ s: String) -> Color {
        switch s.lowercased() {
        case "published", "ok", "sent": return ChirpColor.success
        case "pending", "queued": return ChirpColor.warning
        case "failed", "error": return ChirpColor.danger
        default: return ChirpColor.textSecondary
        }
    }
}
