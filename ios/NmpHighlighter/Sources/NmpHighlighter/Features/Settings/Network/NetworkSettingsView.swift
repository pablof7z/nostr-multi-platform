import SwiftUI

/// Network Settings main screen. Unified list of the user's relays, each row
/// with live status dot + role chips. Taps open `RelayDetailView`; toolbar
/// `+` opens `AddRelaySheet`.
struct NetworkSettingsView: View {
    @Environment(HighlighterStore.self) private var appStore
    @State private var store: NetworkSettingsStore?
    @State private var showAddSheet = false
    @State private var showImportSheet = false
    @State private var pendingRemove: PendingRemove?

    private struct PendingRemove: Identifiable {
        let id = UUID()
        let url: String
        let orphanedRoomNames: [String]
    }

    var body: some View {
        List {
            if let store, !store.isLoading {
                headerSection(store)
                safetySection(store)
                relaysSection(store)
                autoConnectedSection(store)
                actionsSection(store)
                cacheSection(store)
                connectivitySection(store)
                footerSection
            } else {
                ProgressView()
                    .frame(maxWidth: .infinity, alignment: .center)
                    .listRowBackground(Color.clear)
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Network")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    showAddSheet = true
                } label: {
                    Image(systemName: "plus")
                }
                .disabled(store == nil)
            }
        }
        .sheet(isPresented: $showAddSheet) {
            if let store {
                AddRelaySheet { cfg in
                    Task { await store.upsert(cfg) }
                }
            }
        }
        .sheet(isPresented: $showImportSheet) {
            if let store {
                ImportRelaysSheet(store: store)
            }
        }
        .confirmationDialog(
            pendingRemove?.orphanedRoomNames.isEmpty == false
                ? "Remove — you're a member of rooms here"
                : "Remove this relay?",
            isPresented: Binding(
                get: { pendingRemove != nil },
                set: { if !$0 { pendingRemove = nil } }
            ),
            titleVisibility: .visible,
            presenting: pendingRemove
        ) { remove in
            Button("Remove", role: .destructive) {
                Task { await store?.remove(remove.url) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { remove in
            if remove.orphanedRoomNames.isEmpty {
                Text("Highlighter will stop sending and receiving events through \(remove.url).")
            } else {
                Text("This relay hosts \(remove.orphanedRoomNames.count) of your rooms (\(remove.orphanedRoomNames.prefix(3).joined(separator: ", "))\(remove.orphanedRoomNames.count > 3 ? ", …" : "")). Removing it will cut you off from them until you re-add it.")
            }
        }
        .task {
            if store == nil {
                store = NetworkSettingsStore(core: appStore.safeCore)
                appStore.eventBridge?.registerNetworkStore(store!)
            }
            await store?.load()
            store?.startLiveUpdates()
        }
        .onDisappear {
            store?.stopLiveUpdates()
        }
    }

    // MARK: - Sections

    @ViewBuilder
    private func headerSection(_ store: NetworkSettingsStore) -> some View {
        Section {
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 8) {
                    stateDot(
                        allConnected: store.connectedCount == store.relays.count && !store.relays.isEmpty,
                        anyConnected: store.connectedCount > 0
                    )
                    Text(store.aggregateStateLabel)
                        .font(.headline)
                }
                if let err = store.lastError {
                    Text(err)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            .padding(.vertical, 4)
        }
    }

    private func relaysSection(_ store: NetworkSettingsStore) -> some View {
        Section {
            ForEach(store.relays, id: \.url) { row in
                NavigationLink {
                    RelayDetailView(url: row.url, store: store)
                } label: {
                    RelayRowView(
                        config: row,
                        diagnostic: store.diagnostic(for: row.url),
                        nip11: store.nip11(for: row.url)
                    )
                }
            }
            .onDelete { indexSet in
                // Route every delete through the confirmation dialog so the
                // orphan-rooms check applies whether the user swiped or
                // tapped into the detail view.
                for idx in indexSet where idx < store.relays.count {
                    let url = store.relays[idx].url
                    let orphans = orphanedRooms(for: url)
                    pendingRemove = PendingRemove(
                        url: url,
                        orphanedRoomNames: orphans
                    )
                    break // confirm one at a time
                }
            }
        } header: {
            Text("Relays")
        } footer: {
            Text("Your Read and Write relays are published as a kind:10002 event. Other nostr users can see where you read and publish.")
        }
    }

    @ViewBuilder
    private func autoConnectedSection(_ store: NetworkSettingsStore) -> some View {
        if !store.autoConnectedUrls.isEmpty {
            Section {
                ForEach(store.autoConnectedUrls, id: \.self) { url in
                    RelayRowView(
                        config: autoConfig(for: url),
                        diagnostic: store.diagnostic(for: url),
                        nip11: store.nip11(for: url)
                    )
                }
            } header: {
                Text("Auto-connected")
            } footer: {
                Text("Connected to support outbox routing for the people you follow and the hardcoded `purplepag.es` indexer. Not part of your published NIP-65.")
            }
        }
    }

    /// Synthesise a display-only `RelayConfig` for an auto-connected
    /// relay. Outbox-pinned relays carry only Read at the FFI layer, but
    /// the role chips are display-only here anyway — the user can't
    /// toggle them from this section.
    private func autoConfig(for url: String) -> RelayConfig {
        RelayConfig(
            url: url,
            read: true,
            write: false,
            rooms: false,
            indexer: url == "wss://purplepag.es"
        )
    }

    @ViewBuilder
    private func safetySection(_ store: NetworkSettingsStore) -> some View {
        if !store.hasOutbox {
            Section {
                banner(
                    icon: "exclamationmark.triangle.fill",
                    tint: .orange,
                    title: "No outbox relays",
                    detail: "Turn on Write for at least one relay — otherwise your posts won't reach anyone."
                )
            }
        }
        // No indexer banner — `purplepag.es` is hardcoded into the
        // indexer pool by the core (see `relays.rs::PURPLE_PAGES_RELAY`),
        // so profile / follow-list lookups always have somewhere to go.
    }

    private func actionsSection(_ store: NetworkSettingsStore) -> some View {
        Section {
            Button {
                Task { await store.reconnectAll() }
            } label: {
                Label("Reconnect All", systemImage: "arrow.clockwise")
            }
            Button {
                showImportSheet = true
            } label: {
                Label("Import from another user…", systemImage: "person.crop.circle.badge.plus")
            }
        }
    }

    @ViewBuilder
    private func cacheSection(_ store: NetworkSettingsStore) -> some View {
        Section {
            if let stats = store.cacheStats {
                LabeledContent("Events", value: "\(stats.eventCountEstimate)")
                LabeledContent("On disk", value: formatBytes(stats.diskBytes))
            } else {
                HStack {
                    ProgressView().scaleEffect(0.7)
                    Text("Measuring…").foregroundStyle(.secondary).font(.caption)
                }
            }
        } header: {
            Text("Local cache")
        } footer: {
            Text("Everything Highlighter has seen on relays lives here. Uninstall the app to clear it.")
        }
    }

    @ViewBuilder
    private func connectivitySection(_ store: NetworkSettingsStore) -> some View {
        Section {
            Toggle(isOn: Binding(
                get: { store.wifiOnlyEnabled },
                set: { store.setWifiOnly($0) }
            )) {
                Label("Wi-Fi only", systemImage: "wifi")
            }
        } header: {
            Text("Connectivity")
        } footer: {
            Text("When on, Highlighter pauses relay connections on cellular to save mobile data. Resumes automatically on Wi-Fi.")
        }
    }

    private var footerSection: some View {
        Section {
            EmptyView()
        } footer: {
            Text("Tap a relay to see diagnostics, change its roles, or remove it.")
        }
    }

    private func formatBytes(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .binary)
    }

    /// Joined-room names that live on the given relay URL, compared by
    /// trimmed string equality. Used by the remove-confirmation flow.
    private func orphanedRooms(for url: String) -> [String] {
        let target = url.trimmingCharacters(in: .whitespaces)
        return appStore.joinedCommunities
            .filter { $0.relayUrl.trimmingCharacters(in: .whitespaces) == target }
            .map { $0.name.isEmpty ? $0.id : $0.name }
    }

    private func banner(icon: String, tint: Color, title: String, detail: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: icon)
                .foregroundStyle(tint)
                .frame(width: 24, alignment: .center)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.subheadline.weight(.semibold))
                Text(detail).font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private func stateDot(allConnected: Bool, anyConnected: Bool) -> some View {
        Circle()
            .fill(allConnected ? .green : (anyConnected ? .yellow : .red))
            .frame(width: 10, height: 10)
    }
}
