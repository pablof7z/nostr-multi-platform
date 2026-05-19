import Foundation
import Network
import Observation

/// UserDefaults key for the Wi-Fi-only toggle. Persisted outside the
/// @Observable store so it survives process restarts and the `WifiMonitor`
/// can read the initial value before the Store is constructed.
private let wifiOnlyDefaultsKey = "hl.network.wifiOnly"

/// App-scope store for the Network Settings screen. Owns the user's relay
/// rows (config) + the live diagnostics snapshot.
///
/// Architecture contract: nostrdb is the source of truth. `load()` asks the
/// Rust core (which reads from nostrdb / cached kind:10002 + kind:30078);
/// writes go through `HighlighterCore` which publishes new events and
/// reconciles the live pool. Live status deltas arrive via `EventBridge`
/// on the app-scope bus (subscription_id == 0).
@MainActor
@Observable
final class NetworkSettingsStore {
    var relays: [RelayConfig] = []
    var diagnostics: [String: RelayDiagnostic] = [:]
    var nip11ByUrl: [String: Nip11Document] = [:]
    var cacheStats: CacheStats?
    var isLoading: Bool = true
    var lastError: String?
    private(set) var wifiOnlyEnabled: Bool = UserDefaults.standard.bool(forKey: wifiOnlyDefaultsKey)

    @ObservationIgnored private let core: SafeHighlighterCore
    @ObservationIgnored private var pollTask: Task<Void, Never>?
    @ObservationIgnored private var pathMonitor: NWPathMonitor?
    @ObservationIgnored private var inFlightNip11: Set<String> = []

    init(core: SafeHighlighterCore) {
        self.core = core
    }

    /// Index diagnostics by URL for O(1) lookup from row views.
    func diagnostic(for url: String) -> RelayDiagnostic? {
        diagnostics[url]
    }

    /// Cached NIP-11 document for a relay, or `nil` if not yet fetched / the
    /// relay doesn't serve one.
    func nip11(for url: String) -> Nip11Document? {
        nip11ByUrl[url]
    }

    /// URLs of relays in the live pool that the user *didn't* configure —
    /// added by the outbox planner, NIP-77 sync, or the hardcoded purple
    /// indexer pin. We surface them in their own section so the
    /// connected-count math reflects every relay we're actually talking
    /// to (configured + auto-pinned).
    var autoConnectedUrls: [String] {
        let configured = Set(relays.map { $0.url })
        return diagnostics.keys
            .filter { !configured.contains($0) }
            .sorted()
    }

    /// Diagnostics rows for the auto-connected URLs, in the same order.
    var autoConnectedDiagnostics: [RelayDiagnostic] {
        autoConnectedUrls.compactMap { diagnostics[$0] }
    }

    /// Total relays the user can see in the screen — configured + auto.
    var totalVisibleRelays: Int {
        relays.count + autoConnectedUrls.count
    }

    /// Number of relays currently reporting `Connected`. Used for the header
    /// "Online — N of M" pill. Counts every pool relay (configured + auto)
    /// since both groups are visible in the UI.
    var connectedCount: Int {
        diagnostics.values.filter { $0.state == .connected }.count
    }

    /// Human-readable aggregate state for the header pill. The denominator
    /// matches what's actually rendered (configured + auto-connected) so
    /// the user never sees nonsense like "10 of 5".
    var aggregateStateLabel: String {
        let total = totalVisibleRelays
        let online = connectedCount
        if total == 0 { return "No relays" }
        if online == 0 { return "Offline" }
        if online == total { return "Online — \(online) of \(total)" }
        return "\(online) of \(total) online"
    }

    /// True when at least one relay has the `write` flag on. When false,
    /// the user's published events can't reach anyone — show the
    /// no-outbox banner.
    var hasOutbox: Bool { relays.contains { $0.write } }

    /// Kick the pool to attempt a reconnect on every disconnected relay.
    func reconnectAll() async {
        do {
            try await core.reconnectAll()
        } catch {
            lastError = "Couldn't reconnect — \(error)"
        }
    }

    /// Toggle Wi-Fi-only mode. When on, an `NWPathMonitor` suspends the
    /// relay pool on cellular and resumes it when Wi-Fi comes back. The
    /// setting persists in UserDefaults.
    func setWifiOnly(_ on: Bool) {
        wifiOnlyEnabled = on
        UserDefaults.standard.set(on, forKey: wifiOnlyDefaultsKey)
        if on {
            startPathMonitor()
        } else {
            pathMonitor?.cancel()
            pathMonitor = nil
            // Leaving Wi-Fi-only mode → ensure the pool is reconnected
            // regardless of current path state.
            Task { await reconnectAll() }
        }
    }

    // MARK: - Lifecycle

    func load() async {
        do {
            let rows = try await core.getRelays()
            relays = rows
            await refreshDiagnostics()
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
        isLoading = false
        // Fire-and-forget NIP-11 probes for every relay we don't already
        // have cached. Each probe updates `nip11ByUrl` as it resolves, so
        // the rows progressively fill in their icons and names. Fails are
        // silent — a row without a NIP-11 doc just keeps its URL fallback.
        for row in relays where nip11ByUrl[row.url] == nil && !inFlightNip11.contains(row.url) {
            inFlightNip11.insert(row.url)
            let core = self.core
            let url = row.url
            Task { [weak self] in
                defer { Task { @MainActor [weak self] in self?.inFlightNip11.remove(url) } }
                guard let doc = try? await core.probeRelayNip11(url) else { return }
                await MainActor.run { self?.nip11ByUrl[url] = doc }
            }
        }
    }

    func startLiveUpdates() {
        // Already running
        guard pollTask == nil else { return }
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(2))
                guard let self else { return }
                await self.refreshDiagnostics()
            }
        }
        if wifiOnlyEnabled && pathMonitor == nil {
            startPathMonitor()
        }
        Task { await self.refreshCacheStats() }
    }

    func stopLiveUpdates() {
        pollTask?.cancel()
        pollTask = nil
        // Leave the path monitor running — Wi-Fi-only enforcement should
        // keep working after the user leaves the Network screen.
    }

    // MARK: - Cache

    func refreshCacheStats() async {
        if let stats = try? await core.getCacheStats() {
            cacheStats = stats
        }
    }

    // MARK: - NWPathMonitor

    private func startPathMonitor() {
        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { [weak self] path in
            guard let self else { return }
            let isWifi = path.usesInterfaceType(.wifi)
            Task { @MainActor in
                guard self.wifiOnlyEnabled else { return }
                if isWifi {
                    try? await self.core.reconnectAll()
                } else {
                    try? await self.core.disconnectAll()
                }
            }
        }
        monitor.start(queue: .global(qos: .utility))
        pathMonitor = monitor
    }

    // MARK: - Writes

    func upsert(_ cfg: RelayConfig) async {
        do {
            try await core.upsertRelay(cfg)
            await load()
        } catch {
            lastError = "Couldn't add relay — \(error)"
        }
    }

    func remove(_ url: String) async {
        do {
            try await core.removeRelay(url)
            await load()
        } catch {
            lastError = "Couldn't remove relay — \(error)"
        }
    }

    func setRoles(url: String, read: Bool, write: Bool, rooms: Bool, indexer: Bool) async {
        do {
            try await core.setRelayRoles(
                url: url, read: read, write: write, rooms: rooms, indexer: indexer
            )
            await load()
        } catch {
            lastError = "Couldn't update roles — \(error)"
        }
    }

    // MARK: - Delta hook

    /// Called by `EventBridge` on `RelayStatusChanged`. Updates the local
    /// diagnostic for the single relay without reloading the whole list.
    func applyStatus(url: String, state: RelayStatus) {
        if var existing = diagnostics[url] {
            existing.state = state
            diagnostics[url] = existing
        } else {
            diagnostics[url] = RelayDiagnostic(
                url: url,
                state: state,
                rttMs: nil,
                bytesSent: 0,
                bytesReceived: 0,
                connectedSinceTs: nil
            )
        }
    }

    // MARK: - Private

    private func refreshDiagnostics() async {
        do {
            let rows = try await core.getRelayDiagnostics()
            diagnostics = Dictionary(uniqueKeysWithValues: rows.map { ($0.url, $0) })
        } catch {
            // Diagnostics failures are non-fatal — the config rows are still
            // accurate; we just can't show live state this tick.
        }
    }
}
