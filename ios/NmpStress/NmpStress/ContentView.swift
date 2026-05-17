import SwiftUI

struct ContentView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var selectedView = DemoView.timeline
    @State private var route: DemoRoute?

    var body: some View {
        NavigationStack {
            TabView(selection: $selectedView) {
                timeline
                    .tabItem {
                        Label("Timeline", systemImage: "text.bubble")
                    }
                    .tag(DemoView.timeline)

                diagnostics
                    .tabItem {
                        Label("Diagnostics", systemImage: "waveform.path.ecg")
                    }
                    .tag(DemoView.diagnostics)
            }
            .navigationTitle(selectedView.title)
            .navigationBarTitleDisplayMode(.large)
            .navigationDestination(item: $route) { route in
                switch route {
                case let .author(pubkey):
                    ProfileDetailView(pubkey: pubkey)
                case let .thread(eventID):
                    ThreadDetailView(eventID: eventID)
                }
            }
            .toolbar {
                ToolbarItemGroup(placement: .topBarTrailing) {
                    Button {
                        model.resetAndRestart()
                    } label: {
                        Image(systemName: "arrow.clockwise")
                    }
                    .accessibilityLabel("Refresh")
                    .accessibilityIdentifier("demo-refresh")

                    Button {
                        model.isRunning ? model.stop() : model.start()
                    } label: {
                        Image(systemName: model.isRunning ? "pause.fill" : "play.fill")
                    }
                    .accessibilityLabel(model.isRunning ? "Stop" : "Start")
                    .accessibilityIdentifier("demo-toggle")
                }
            }
            .onChange(of: model.visibleLimit) { _, _ in model.applyConfiguration() }
            .onChange(of: model.emitHz) { _, _ in model.applyConfiguration() }
            .task {
                model.start()
            }
        }
    }

    private var statusSummary: some View {
        VStack(spacing: 14) {
            connectionHeader
            Divider()
            metrics
        }
        .padding(16)
        .nmpGlassPanel(cornerRadius: 28)
    }

    private var connectionHeader: some View {
        HStack(spacing: 12) {
            statusDot
            VStack(alignment: .leading, spacing: 2) {
                Text(model.relayStatus?.connection.uppercased() ?? "STARTING")
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                    .minimumScaleFactor(0.75)
                    .accessibilityIdentifier("relay-state-value")
                Text(model.relayUrl)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .layoutPriority(1)
            Spacer()
            VStack(alignment: .trailing, spacing: 2) {
                Text("\(model.relayStatus?.activeWireSubscriptions ?? 0)")
                    .font(.caption.monospacedDigit().weight(.semibold))
                    .accessibilityIdentifier("wire-sub-count-value")
                Text("REQs")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            VStack(alignment: .trailing, spacing: 2) {
                Text(format(model.metrics?.timelineAuthors))
                    .font(.caption.monospacedDigit().weight(.semibold))
                    .accessibilityIdentifier("timeline-authors-value")
                Text("authors")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var statusDot: some View {
        Circle()
            .fill(statusColor)
            .frame(width: 10, height: 10)
            .accessibilityIdentifier("relay-status-dot")
    }

    private var statusColor: Color {
        switch model.relayStatus?.connection {
        case "connected":
            .green
        case "connecting":
            .yellow
        case "backing_off":
            .orange
        case "closed", "offline":
            .red
        default:
            .gray
        }
    }

    private var metrics: some View {
        let metrics = model.metrics
        return Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 8) {
            GridRow {
                MetricCell("rev", "\(model.rev)", valueID: "metric-rev-value")
                MetricCell("events", format(metrics?.eventsRx), valueID: "metric-events-value")
                MetricCell("visible", format(metrics?.visibleItems), valueID: "metric-visible-value")
            }
            GridRow {
                MetricCell("profiles", format(metrics?.visibleProfiledItems), valueID: "metric-profiled-value")
                MetricCell("payload", bytes(metrics?.payloadBytes), valueID: "metric-payload-value")
                MetricCell("rx", bytes(metrics?.bytesRx), valueID: "metric-rx-value")
            }
            GridRow {
                MetricCell("firehose", format(metrics?.diagnosticFirehoseEvents), valueID: "metric-firehose-value")
                MetricCell("max batch", format(metrics?.maxEventsPerUpdate), valueID: "metric-max-batch-value")
                MetricCell("max r→rust", millis(metrics?.maxEventToEmitMs), valueID: "metric-max-relay-rust-value")
            }
            GridRow {
                MetricCell("first", millis(metrics?.firstEventMs), valueID: "metric-first-ms-value")
                MetricCell("profile", millis(metrics?.targetProfileLoadedMs), valueID: "metric-profile-ms-value")
                MetricCell("batch", format(metrics?.eventsSinceLastUpdate), valueID: "metric-batch-value")
            }
            GridRow {
                MetricCell("relay→rust", millis(metrics?.lastEventToEmitMs), valueID: "metric-relay-rust-value")
                MetricCell("cb→screen", "\(model.appMetrics.lastCallbackToAppliedMicros) us", valueID: "metric-callback-screen-value")
                MetricCell("max apply", "\(model.appMetrics.maxApplyMicros) us", valueID: "metric-apply-us-value")
            }
        }
        .font(.caption)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var timeline: some View {
        List {
            Section {
                statusSummary
                    .listRowInsets(EdgeInsets(top: 6, leading: 16, bottom: 6, trailing: 16))
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            }

            if let profile = model.profile {
                Section("Profile") {
                    ProfileCardView(profile: profile)
                        .accessibilityIdentifier("slice-profile-card")
                }
            }

            Section("Timeline") {
                ForEach(model.items) { item in
                    TimelineRow(
                        item: item,
                        openAuthor: { route = .author(item.authorPubkey) },
                        openThread: { route = .thread(item.id) }
                    )
                    .accessibilityIdentifier("timeline-row-\(item.id)")
                }
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(Color(uiColor: .systemGroupedBackground))
        .refreshable {
            model.resetAndRestart()
        }
        .accessibilityIdentifier("timeline-list")
    }

    private var diagnostics: some View {
        List {
            if !model.relayStatuses.isEmpty {
                Section("Relays") {
                    ForEach(model.relayStatuses, id: \.role) { relay in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text(relay.role.capitalized)
                                    .font(.subheadline.weight(.semibold))
                                Spacer()
                                Text(relay.connection)
                                    .font(.caption.monospacedDigit())
                                    .foregroundStyle(.secondary)
                            }
                            Text(relay.relayUrl)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            HStack {
                                Text("\(relay.activeWireSubscriptions) REQs")
                                Spacer()
                                Text(bytes(relay.bytesRx))
                            }
                            .font(.caption2.monospacedDigit())
                            .foregroundStyle(.secondary)
                            if let notice = relay.lastNotice {
                                Text(notice)
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }
                            if let error = relay.lastError {
                                Text(error)
                                    .font(.caption2)
                                    .foregroundStyle(.red)
                            }
                        }
                    }
                }
            } else if let relay = model.relayStatus {
                Section("Relay") {
                    DiagnosticRow("URL", relay.relayUrl)
                    DiagnosticRow("Connection", relay.connection)
                    DiagnosticRow("Auth", relay.auth)
                    DiagnosticRow("NIP-77", relay.nip77Negentropy)
                    DiagnosticRow("Reconnects", "\(relay.reconnectCount)")
                    DiagnosticRow("RX", bytes(relay.bytesRx))
                    DiagnosticRow("TX", bytes(relay.bytesTx))
                    if let notice = relay.lastNotice {
                        DiagnosticRow("Notice", notice)
                    }
                    if let error = relay.lastError {
                        DiagnosticRow("Error", error)
                    }
                }
            }

            Section("Logical Interests") {
                ForEach(model.logicalInterests) { interest in
                    VStack(alignment: .leading, spacing: 4) {
                        Text(interest.key)
                            .font(.subheadline.weight(.semibold))
                        Text("\(interest.state) · ref \(interest.refcount) · \(interest.cacheCoverage)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }

            Section("Wire Subscriptions") {
                ForEach(model.wireSubscriptions) { sub in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(sub.wireId)
                                .font(.subheadline.weight(.semibold))
                            Spacer()
                            Text(sub.state)
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                        Text(sub.filterSummary)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }

            Section("Runtime Log") {
                ForEach(model.logs.reversed(), id: \.self) { log in
                    Text(log)
                        .font(.caption.monospaced())
                        .textSelection(.enabled)
                }
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(Color(uiColor: .systemGroupedBackground))
        .accessibilityIdentifier("diagnostics-list")
    }
}
