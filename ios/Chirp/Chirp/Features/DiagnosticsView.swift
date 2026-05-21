import SwiftUI

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
                        .fill(model.isRunning ? .green : .red)
                        .frame(width: 8, height: 8)
                    Text(model.isRunning ? "Running" : "Stopped")
                        .font(.callout.weight(.medium))
                        .foregroundStyle(model.isRunning ? .green : .red)
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
    private var allRelayURLs: [String] {
        var seen = Set<String>()
        var urls: [String] = []
        for url in model.relayStatuses.map(\.relayUrl) + model.wireSubscriptions.map(\.relayUrl) {
            if seen.insert(url).inserted { urls.append(url) }
        }
        return urls
    }
    private func syntheticRelayStatus(url: String, subs: [WireSubscriptionStatus]) -> RelayStatus {
        let activeSubs = subs.filter { ["open", "live", "active", "opening"].contains($0.state) }.count
        return RelayStatus(
            role: "outbox",
            relayUrl: url,
            connection: activeSubs > 0 ? "connected" : "unknown",
            auth: "—",
            nip77Negentropy: nil,
            activeWireSubscriptions: activeSubs,
            reconnectCount: 0,
            lastConnectedAtMs: nil,
            lastEventAtMs: subs.compactMap(\.lastEventAtMs).max(),
            lastNotice: nil,
            lastError: nil,
            bytesRx: nil,
            bytesTx: nil
        )
    }
    private var relaySection: some View {
        let urls = allRelayURLs
        return Section("Relays (\(urls.count))") {
            if urls.isEmpty {
                Text("No relays yet")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(urls, id: \.self) { url in
                    let status = model.relayStatuses.first(where: { $0.relayUrl == url })
                    let subs = model.wireSubscriptions.filter { $0.relayUrl == url }
                    let interests = model.logicalInterests.filter { $0.relayUrls.contains(url) }
                    NavigationLink(destination: RelayDetailView(
                        relay: status ?? syntheticRelayStatus(url: url, subs: subs),
                        wireSubscriptions: subs,
                        logicalInterests: interests
                    )) {
                        DiagRelayRow(relay: status ?? syntheticRelayStatus(url: url, subs: subs))
                    }
                }
            }
        }
    }
    @ViewBuilder
    private var logicalInterestsSection: some View {
        if !model.logicalInterests.isEmpty {
            Section("Logical Interests (\(model.logicalInterests.count))") {
                ForEach(model.logicalInterests) { interest in
                    VStack(alignment: .leading, spacing: 4) {
                        Text(interest.key)
                            .font(.body.monospaced())
                            .lineLimit(1)
                        HStack(spacing: 12) {
                            Text(interest.state)
                                .font(.caption)
                                .foregroundStyle(interestStateColor(interest.state))
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
    private func interestStateColor(_ state: String) -> Color {
        switch state {
        case "active", "warming": return .green
        case "idle": return .secondary
        default: return .orange
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
            if let activeID = model.activeAccount,
               let account = model.accounts.first(where: { $0.id == activeID }) {
                HStack {
                    Text("Display")
                    Spacer()
                    Text(account.displayName.isEmpty ? "—" : account.displayName)
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
                        .foregroundStyle(account.isActive ? .green : .secondary)
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
                                .foregroundStyle(copiedNpub ? .green : Color.accentColor)
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
                ForEach(model.logs.reversed().prefix(50), id: \.self) { entry in
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
                .foregroundStyle(Color.accentColor)
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
private struct DiagRelayRow: View {
    let relay: RelayStatus
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .center, spacing: 8) {
                Circle()
                    .fill(connectionColor)
                    .frame(width: 8, height: 8)
                Text(relay.relayUrl)
                    .font(.body.monospaced())
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 0)
                Text(relay.role.capitalized)
                    .font(.caption2.weight(.bold))
                    .foregroundStyle(roleColor)
            }
            HStack(spacing: 12) {
                Text(relay.connection.capitalized)
                    .font(.caption)
                    .foregroundStyle(connectionColor)
                Text("Auth: \(relay.auth)")
                    .font(.caption)
                    .foregroundStyle(authColor)
                Text("\(relay.activeWireSubscriptions) subs")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if relay.reconnectCount > 0 {
                    Text("↩ \(relay.reconnectCount)")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
            }
            if let notice = relay.lastNotice {
                Text(notice)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            if let error = relay.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 4)
    }
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
    private func statusColor(_ s: String) -> Color {
        switch s.lowercased() {
        case "published", "ok", "sent": return .green
        case "pending", "queued": return .orange
        case "failed", "error": return .red
        default: return .secondary
        }
    }
}
