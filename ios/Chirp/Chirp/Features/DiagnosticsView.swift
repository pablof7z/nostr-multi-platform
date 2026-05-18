import SwiftUI

struct DiagnosticsView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var copiedNpub = false

    var body: some View {
        ScrollView {
            VStack(spacing: ChirpSpace.xl) {
                kernelSection
                perfSection
                metricsSection
                relaySection
                logicalInterestsSection
                wireSubscriptionsSection
                publishQueueSection
                accountSection
                runtimeLogSection
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.vertical, ChirpSpace.xl)
        }
        .accessibilityIdentifier("diagnostics-list")
        .background(Color(.systemBackground))
        .navigationTitle("Diagnostics")
        .navigationBarTitleDisplayMode(.large)
    }

    // ── Kernel heartbeat ──────────────────────────────────────────────────

    private var kernelSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Kernel")
            GlassCard {
                VStack(spacing: 0) {
                    DiagRow(label: "Status") {
                        HStack(spacing: ChirpSpace.xs) {
                            Circle()
                                .fill(model.isRunning ? ChirpColor.positive : ChirpColor.like)
                                .frame(width: 8, height: 8)
                                .shadow(color: model.isRunning ? ChirpColor.positive.opacity(0.6) : .clear,
                                        radius: 4)
                            Text(model.isRunning ? "Running" : "Stopped")
                                .font(ChirpFont.callout.weight(.medium))
                                .foregroundStyle(model.isRunning ? ChirpColor.positive : ChirpColor.like)
                        }
                        .animation(.easeInOut(duration: 0.3), value: model.isRunning)
                    }

                    DiagDivider()

                    DiagRow(label: "Connection") {
                        let conn = model.relayStatuses.first?.connection.uppercased() ?? "STARTING"
                        Text(conn)
                            .font(ChirpFont.callout.weight(.semibold))
                            .foregroundStyle(connectionColor(conn))
                            .accessibilityIdentifier("relay-state-value")
                    }

                    DiagDivider()

                    DiagRow(label: "Rev") {
                        Text("\(model.rev)")
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textPrimary)
                            .monospacedDigit()
                            .contentTransition(.numericText())
                            .animation(.smooth(duration: 0.25), value: model.rev)
                    }

                    DiagDivider()

                    DiagRow(label: "Snapshots") {
                        Text("\(model.snapshotCount)")
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textPrimary)
                            .monospacedDigit()
                            .contentTransition(.numericText())
                            .animation(.smooth(duration: 0.25), value: model.snapshotCount)
                    }

                    DiagDivider()

                    DiagRow(label: "Last Snapshot") {
                        if let date = model.lastSnapshotAt {
                            Text(date, style: .relative)
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textSecondary)
                        } else {
                            Text("—")
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textTertiary)
                        }
                    }

                    DiagDivider()

                    DiagRow(label: "Update Seq") {
                        if let seq = model.metrics?.updateSequence {
                            Text("\(seq)")
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textPrimary)
                                .monospacedDigit()
                                .contentTransition(.numericText())
                                .animation(.smooth(duration: 0.25), value: seq)
                        } else {
                            Text("—")
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textTertiary)
                        }
                    }

                    DiagDivider()

                    DiagRow(label: "Relay") {
                        Text(model.relayUrl.isEmpty ? "—" : model.relayUrl)
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textSecondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                }
            }
        }
    }

    private func connectionColor(_ conn: String) -> Color {
        let s = conn.lowercased()
        if s == "connected" { return ChirpColor.positive }
        if s.contains("connect") { return ChirpColor.zap }
        return ChirpColor.like
    }

    // ── Swift-side timing (NmpStress perf goals) ──────────────────────────

    private var perfSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Performance")
            GlassCard {
                VStack(spacing: 0) {
                    DiagRow(label: "Events Rx") {
                        Text(model.metrics.map { "\($0.eventsRx)" } ?? "—")
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textPrimary)
                            .monospacedDigit()
                            .contentTransition(.numericText())
                            .accessibilityIdentifier("metric-events-value")
                    }

                    DiagDivider()

                    DiagRow(label: "Visible") {
                        Text(model.metrics.map { "\($0.visibleItems)" } ?? "—")
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textPrimary)
                            .monospacedDigit()
                            .contentTransition(.numericText())
                            .accessibilityIdentifier("metric-visible-value")
                    }

                    DiagDivider()

                    DiagRow(label: "Bytes Rx") {
                        let value = model.metrics.map { formatBytes(Int64($0.bytesRx)) } ?? "—"
                        Text(value)
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textSecondary)
                            .accessibilityIdentifier("metric-rx-value")
                    }

                    DiagDivider()

                    DiagRow(label: "First Event") {
                        let value = model.metrics?.firstEventMs.map { "\($0) ms" } ?? "-"
                        Text(value)
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textSecondary)
                            .accessibilityIdentifier("metric-first-ms-value")
                    }

                    DiagDivider()

                    DiagRow(label: "Max Apply") {
                        Text("\(model.appMetrics.maxApplyMicros) us")
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textSecondary)
                            .accessibilityIdentifier("metric-apply-us-value")
                    }

                    DiagDivider()

                    DiagRow(label: "Decode") {
                        Text("\(model.appMetrics.lastDecodeMicros) us")
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textTertiary)
                    }

                    DiagDivider()

                    DiagRow(label: "cb→screen") {
                        Text("\(model.appMetrics.lastCallbackToAppliedMicros) us")
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textTertiary)
                    }

                    DiagDivider()

                    DiagRow(label: "Queue Depth") {
                        Text(model.metrics.map { "\($0.actorQueueDepth)" } ?? "—")
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textTertiary)
                    }
                }
            }
        }
    }

    // ── Metrics ───────────────────────────────────────────────────────────

    private var metricsSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Metrics")
            if let m = model.metrics {
                HStack(spacing: ChirpSpace.m) {
                    MetricTile(
                        label: "Stored",
                        value: m.storedEvents.formatted(.number.notation(.compactName)),
                        icon: "internaldrive",
                        color: ChirpColor.accent
                    )
                    MetricTile(
                        label: "Visible",
                        value: m.visibleItems.formatted(.number.notation(.compactName)),
                        icon: "eye",
                        color: ChirpColor.zap
                    )
                    MetricTile(
                        label: "Events Rx",
                        value: m.eventsRx.formatted(.number.notation(.compactName)),
                        icon: "arrow.down.circle",
                        color: ChirpColor.positive
                    )
                }
                HStack(spacing: ChirpSpace.m) {
                    MetricTile(
                        label: "Note Events",
                        value: m.noteEvents.formatted(.number.notation(.compactName)),
                        icon: "doc.text",
                        color: ChirpColor.textSecondary
                    )
                    MetricTile(
                        label: "Queue Depth",
                        value: "\(m.actorQueueDepth)",
                        icon: "tray.full",
                        color: m.actorQueueDepth > 100 ? ChirpColor.like : ChirpColor.textTertiary
                    )
                    MetricTile(
                        label: "Payload",
                        value: formatBytes(Int64(m.payloadBytes)),
                        icon: "doc.zipper",
                        color: ChirpColor.textSecondary
                    )
                }
                .animation(.smooth(duration: 0.25), value: m.storedEvents)
            } else {
                GlassCard {
                    HStack {
                        ProgressView()
                            .tint(ChirpColor.accent)
                        Text("Waiting for kernel…")
                            .font(ChirpFont.callout)
                            .foregroundStyle(ChirpColor.textSecondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, ChirpSpace.xs)
                }
            }
        }
    }

    // ── Relay status table ────────────────────────────────────────────────

    private var relaySection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Relays (\(model.relayStatuses.count))")
            if model.relayStatuses.isEmpty {
                GlassCard {
                    Text("No relay statuses yet")
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textTertiary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, ChirpSpace.xs)
                }
            } else {
                GlassCard {
                    VStack(spacing: 0) {
                        ForEach(Array(model.relayStatuses.enumerated()), id: \.element.id) { index, relay in
                            DiagRelayRow(relay: relay)
                            if index < model.relayStatuses.count - 1 {
                                DiagDivider()
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Logical interests (NmpStress perf goal) ───────────────────────────

    @ViewBuilder
    private var logicalInterestsSection: some View {
        if !model.logicalInterests.isEmpty {
            VStack(alignment: .leading, spacing: ChirpSpace.m) {
                ChirpSectionHeader(title: "Logical Interests (\(model.logicalInterests.count))")
                GlassCard {
                    VStack(spacing: 0) {
                        ForEach(Array(model.logicalInterests.enumerated()), id: \.element.id) { index, interest in
                            VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                                Text(interest.key)
                                    .font(ChirpFont.mono)
                                    .foregroundStyle(ChirpColor.textPrimary)
                                    .lineLimit(1)
                                HStack(spacing: ChirpSpace.s) {
                                    DiagChip(label: interest.state, color: interestStateColor(interest.state))
                                    DiagChip(label: "ref \(interest.refcount)", color: ChirpColor.textTertiary)
                                    DiagChip(label: interest.cacheCoverage, color: ChirpColor.accent)
                                }
                            }
                            .padding(.vertical, ChirpSpace.s)
                            if index < model.logicalInterests.count - 1 {
                                DiagDivider()
                            }
                        }
                    }
                }
            }
        }
    }

    private func interestStateColor(_ state: String) -> Color {
        switch state {
        case "active", "warming": return ChirpColor.positive
        case "idle": return ChirpColor.textTertiary
        default: return ChirpColor.zap
        }
    }

    // ── Wire subscriptions (NmpStress perf goal) ──────────────────────────

    @ViewBuilder
    private var wireSubscriptionsSection: some View {
        if !model.wireSubscriptions.isEmpty {
            VStack(alignment: .leading, spacing: ChirpSpace.m) {
                ChirpSectionHeader(title: "Wire Subscriptions (\(model.wireSubscriptions.count))")
                GlassCard {
                    VStack(spacing: 0) {
                        ForEach(Array(model.wireSubscriptions.enumerated()), id: \.element.id) { index, sub in
                            VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                                HStack {
                                    Text(sub.wireId)
                                        .font(ChirpFont.mono)
                                        .foregroundStyle(ChirpColor.textPrimary)
                                        .lineLimit(1)
                                    Spacer(minLength: 0)
                                    DiagChip(label: sub.state, color: subStateColor(sub.state))
                                }
                                Text(sub.filterSummary)
                                    .font(ChirpFont.caption)
                                    .foregroundStyle(ChirpColor.textSecondary)
                                    .lineLimit(2)
                                Text(sub.relayUrl)
                                    .font(ChirpFont.caption)
                                    .foregroundStyle(ChirpColor.textTertiary)
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                            }
                            .padding(.vertical, ChirpSpace.s)
                            if index < model.wireSubscriptions.count - 1 {
                                DiagDivider()
                            }
                        }
                    }
                }
            }
        }
    }

    private func subStateColor(_ state: String) -> Color {
        switch state {
        case "open", "active": return ChirpColor.positive
        case "closed": return ChirpColor.textTertiary
        default: return ChirpColor.zap
        }
    }

    // ── Publish queue ─────────────────────────────────────────────────────

    @ViewBuilder
    private var publishQueueSection: some View {
        if !model.publishQueue.isEmpty {
            VStack(alignment: .leading, spacing: ChirpSpace.m) {
                ChirpSectionHeader(title: "Publish Queue (\(model.publishQueue.count))")
                GlassCard {
                    VStack(spacing: 0) {
                        ForEach(Array(model.publishQueue.enumerated()), id: \.element.id) { index, entry in
                            DiagPublishRow(entry: entry)
                            if index < model.publishQueue.count - 1 {
                                DiagDivider()
                            }
                        }
                    }
                }
            }
            .transition(.move(edge: .top).combined(with: .opacity))
            .animation(.smooth, value: model.publishQueue.count)
        }
    }

    // ── Active account ────────────────────────────────────────────────────

    private var accountSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Active Account")
            GlassCard {
                VStack(spacing: ChirpSpace.m) {
                    if let activeID = model.activeAccount,
                       let account = model.accounts.first(where: { $0.id == activeID }) {
                        VStack(spacing: 0) {
                            DiagRow(label: "Display") {
                                Text(account.displayName.isEmpty ? "—" : account.displayName)
                                    .font(ChirpFont.callout)
                                    .foregroundStyle(ChirpColor.textPrimary)
                            }
                            DiagDivider()
                            DiagRow(label: "Signer") {
                                Text(account.signerKind)
                                    .font(ChirpFont.mono)
                                    .foregroundStyle(ChirpColor.textSecondary)
                            }
                            DiagDivider()
                            DiagRow(label: "Status") {
                                Text(account.status)
                                    .font(ChirpFont.caption.weight(.semibold))
                                    .foregroundStyle(account.isActive ? ChirpColor.positive : ChirpColor.textTertiary)
                            }
                        }

                        VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                            Text("npub")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(ChirpColor.textTertiary)
                                .tracking(0.5)
                            Button {
                                UIPasteboard.general.string = account.npub
                                let generator = UIImpactFeedbackGenerator(style: .light)
                                generator.impactOccurred()
                                withAnimation(.smooth(duration: 0.2)) { copiedNpub = true }
                                Task {
                                    try? await Task.sleep(for: .seconds(2))
                                    withAnimation(.smooth(duration: 0.3)) { copiedNpub = false }
                                }
                            } label: {
                                HStack(spacing: ChirpSpace.s) {
                                    Text(account.npub)
                                        .font(ChirpFont.mono)
                                        .foregroundStyle(ChirpColor.textSecondary)
                                        .lineLimit(2)
                                        .multilineTextAlignment(.leading)
                                    Spacer(minLength: 0)
                                    Image(systemName: copiedNpub ? "checkmark.circle.fill" : "doc.on.doc")
                                        .font(.system(size: 14, weight: .medium))
                                        .foregroundStyle(copiedNpub ? ChirpColor.positive : ChirpColor.accent)
                                        .animation(.smooth(duration: 0.2), value: copiedNpub)
                                }
                                .padding(ChirpSpace.m)
                                .background(ChirpColor.accentSoft, in: RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall, style: .continuous))
                            }
                            .buttonStyle(.plain)
                        }
                    } else {
                        HStack(spacing: ChirpSpace.s) {
                            Image(systemName: "person.slash")
                                .foregroundStyle(ChirpColor.textTertiary)
                            Text("No active account")
                                .font(ChirpFont.callout)
                                .foregroundStyle(ChirpColor.textTertiary)
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, ChirpSpace.xs)
                    }
                }
            }
        }
    }

    // ── Runtime log (NmpStress perf goal) ────────────────────────────────

    @ViewBuilder
    private var runtimeLogSection: some View {
        if !model.logs.isEmpty {
            VStack(alignment: .leading, spacing: ChirpSpace.m) {
                ChirpSectionHeader(title: "Runtime Log (\(model.logs.count))")
                GlassCard {
                    VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                        ForEach(model.logs.reversed().prefix(50), id: \.self) { entry in
                            Text(entry)
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textSecondary)
                                .fixedSize(horizontal: false, vertical: true)
                                .textSelection(.enabled)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
    }

    // ── Byte formatter ────────────────────────────────────────────────────

    private func formatBytes(_ bytes: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: bytes, countStyle: .binary)
    }
}

// ── Subcomponents ─────────────────────────────────────────────────────────

/// Single key/value diagnostic row.
private struct DiagRow<Value: View>: View {
    let label: String
    @ViewBuilder var value: Value

    var body: some View {
        HStack(alignment: .center) {
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

/// Hairline separator inside cards.
private struct DiagDivider: View {
    var body: some View {
        Divider()
            .background(ChirpColor.hairline)
    }
}

/// Compact metric tile with icon + value + label.
private struct MetricTile: View {
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
                    .contentTransition(.numericText())
                Text(label)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, ChirpSpace.xs)
        }
    }
}

/// One row in the relay status table.
private struct DiagRelayRow: View {
    let relay: RelayStatus

    var body: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.xs) {
            HStack(alignment: .center, spacing: ChirpSpace.s) {
                Circle()
                    .fill(connectionColor)
                    .frame(width: 8, height: 8)
                    .shadow(color: connectionColor.opacity(0.6), radius: 3)

                Text(relay.relayUrl)
                    .font(ChirpFont.mono)
                    .foregroundStyle(ChirpColor.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.middle)

                Spacer(minLength: 0)

                Text(relay.role.capitalized)
                    .font(.system(.caption2, design: .rounded).weight(.bold))
                    .foregroundStyle(roleColor)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 3)
                    .background(roleColor.opacity(0.15), in: Capsule())
            }

            HStack(spacing: ChirpSpace.m) {
                DiagChip(label: relay.connection.capitalized, color: connectionColor)
                DiagChip(label: "Auth: \(relay.auth)", color: authColor)
                DiagChip(label: "\(relay.activeWireSubscriptions) subs", color: ChirpColor.textTertiary)
                if relay.reconnectCount > 0 {
                    DiagChip(label: "↩ \(relay.reconnectCount)", color: ChirpColor.zap)
                }
            }

            if let notice = relay.lastNotice {
                Text(notice)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)
                    .lineLimit(2)
            }
            if let error = relay.lastError {
                Text(error)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.like)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, ChirpSpace.s)
    }

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
}

/// Tiny pill label for relay chips.
private struct DiagChip: View {
    let label: String
    let color: Color

    var body: some View {
        Text(label)
            .font(.system(.caption2, design: .rounded).weight(.medium))
            .foregroundStyle(color)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(color.opacity(0.1), in: Capsule())
    }
}

/// One row in the publish queue.
private struct DiagPublishRow: View {
    let entry: PublishQueueEntry

    var body: some View {
        HStack(spacing: ChirpSpace.m) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: ChirpSpace.xs) {
                    Text(shortID(entry.eventId))
                        .font(ChirpFont.mono)
                        .foregroundStyle(ChirpColor.textPrimary)
                    Text("kind \(entry.kind)")
                        .font(ChirpFont.caption)
                        .foregroundStyle(ChirpColor.textTertiary)
                }
                Text("\(entry.targetRelays) relay\(entry.targetRelays == 1 ? "" : "s")")
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textSecondary)
            }
            Spacer()
            Text(entry.status.capitalized)
                .font(.system(.caption2, design: .rounded).weight(.semibold))
                .foregroundStyle(statusColor(entry.status))
                .padding(.horizontal, ChirpSpace.s)
                .padding(.vertical, 3)
                .background(statusColor(entry.status).opacity(0.15), in: Capsule())
        }
        .padding(.vertical, ChirpSpace.s)
    }

    private func shortID(_ id: String) -> String {
        guard id.count >= 12 else { return id }
        return "\(id.prefix(8))…"
    }

    private func statusColor(_ s: String) -> Color {
        switch s.lowercased() {
        case "published", "ok", "sent": return ChirpColor.positive
        case "pending", "queued": return ChirpColor.zap
        case "failed", "error": return ChirpColor.like
        default: return ChirpColor.textTertiary
        }
    }
}
