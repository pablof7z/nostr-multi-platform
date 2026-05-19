import Foundation
import Observation

/// Drives `SearchView`. Owns the debounced query pipeline, the four result
/// buckets (highlights / articles / communities / people), and the live
/// NIP-50 subscription whose deltas re-run the local article match so
/// relay-delivered events fade into the Articles section as they arrive.
///
/// Architecture note: every bucket is read from nostrdb via the Rust core —
/// the NIP-50 relay sub just ingests into ndb, which in turn triggers a
/// `SearchArticlesUpdated` delta that the store reacts to by re-running
/// `search_articles` locally. NostrDB stays the only source of truth.
@MainActor
@Observable
final class SearchStore {
    // MARK: - Inputs

    /// Raw text from the search field. Writes schedule a debounced query.
    var query: String = "" {
        didSet { scheduleSearch(for: query) }
    }

    // MARK: - Outputs (reactive)

    private(set) var highlights: [HighlightRecord] = []
    private(set) var articles: [ArticleRecord] = []
    private(set) var communities: [CommunitySummary] = []
    private(set) var profiles: [ProfileMetadata] = []

    /// True while a local scan is running for the current query — flickers to
    /// avoid a blank frame on a fresh query.
    private(set) var isLocalLoading: Bool = false
    /// True while at least one NIP-50 relay subscription is still settling for
    /// the current query (first reply hasn't arrived OR just arrived within
    /// the last second). Drives a quiet "searching the web" affordance.
    private(set) var isRelayLoading: Bool = false

    /// The resolved set of relays the NIP-50 query is hitting. Rendered as a
    /// subtle footnote under the Articles section so the user can see their
    /// configured NIP-51 search relays are actually in use.
    private(set) var searchRelays: [String] = []

    // MARK: - Dependencies

    private let safeCore: SafeHighlighterCore
    private let eventBridge: EventBridge?

    // MARK: - Internal state

    /// Monotonically increasing token — every scheduled query bumps it so
    /// in-flight callbacks for a stale query can no-op.
    private var searchToken: UInt64 = 0
    /// Most-recent applied query (the one whose results populate the buckets).
    private var appliedQuery: String = ""
    private var debounceTask: Task<Void, Never>?
    private var activeSearchHandle: UInt64?
    /// Query the current NIP-50 subscription was opened with. If the user
    /// edits the query, we tear down + re-open.
    private var activeRelayQuery: String = ""
    private var relayLoadingResetTask: Task<Void, Never>?

    // MARK: - Init

    init(safeCore: SafeHighlighterCore, eventBridge: EventBridge?) {
        self.safeCore = safeCore
        self.eventBridge = eventBridge
    }

    // MARK: - Lifecycle

    func start() async {
        if let relays = try? await safeCore.getSearchRelays() {
            searchRelays = relays
        }
    }

    func stop() {
        debounceTask?.cancel()
        debounceTask = nil
        relayLoadingResetTask?.cancel()
        relayLoadingResetTask = nil
        if let handle = activeSearchHandle {
            Task { [safeCore, eventBridge] in
                await safeCore.unsubscribe(handle)
                eventBridge?.unregister(handle: handle)
            }
            activeSearchHandle = nil
        }
    }

    // MARK: - Query orchestration

    /// Re-applies a search explicitly (e.g. tapping a recent search chip).
    func submit(_ query: String) {
        self.query = query
        // Fire immediately, skipping the debounce.
        debounceTask?.cancel()
        let token = bumpToken()
        Task { await runSearch(for: query, token: token) }
    }

    func clear() {
        debounceTask?.cancel()
        debounceTask = nil
        query = ""
        appliedQuery = ""
        highlights = []
        articles = []
        communities = []
        profiles = []
        isLocalLoading = false
        isRelayLoading = false
        tearDownRelaySearch()
    }

    private func scheduleSearch(for q: String) {
        debounceTask?.cancel()
        let trimmed = q.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            appliedQuery = ""
            highlights = []
            articles = []
            communities = []
            profiles = []
            isLocalLoading = false
            isRelayLoading = false
            tearDownRelaySearch()
            return
        }
        isLocalLoading = true
        let token = bumpToken()
        debounceTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 220_000_000) // 220ms
            if Task.isCancelled { return }
            guard let self else { return }
            await self.runSearch(for: trimmed, token: token)
        }
    }

    private func bumpToken() -> UInt64 {
        searchToken &+= 1
        return searchToken
    }

    private func runSearch(for q: String, token: UInt64) async {
        async let h = try? safeCore.searchHighlights(query: q, limit: 30)
        async let a = try? safeCore.searchArticles(query: q, limit: 30)
        async let c = try? safeCore.searchCommunities(query: q, limit: 20)
        async let p = try? safeCore.searchProfiles(query: q, limit: 20)
        let (hs, ars, cs, ps) = await (h, a, c, p)

        guard token == searchToken else { return }

        appliedQuery = q
        highlights = hs ?? []
        articles = ars ?? []
        communities = cs ?? []
        profiles = ps ?? []
        isLocalLoading = false

        if activeRelayQuery != q {
            await refreshRelaySubscription(for: q)
        }
    }

    // MARK: - NIP-50 relay subscription

    private func refreshRelaySubscription(for q: String) async {
        tearDownRelaySearch()
        activeRelayQuery = q
        isRelayLoading = true
        do {
            let handle = try await safeCore.subscribeArticleSearch(query: q)
            if appliedQuery != q {
                // Query moved on while we were opening — tear down immediately.
                await safeCore.unsubscribe(handle)
                return
            }
            activeSearchHandle = handle
            eventBridge?.registerSearch(self, handle: handle)
        } catch {
            isRelayLoading = false
        }

        // Relay results may trickle in over a few seconds. The spinner stops
        // on the first delta (`applyRelaySearchUpdate`) or, barring that,
        // after a safety timeout so the UI doesn't hang on a quiet relay.
        relayLoadingResetTask?.cancel()
        relayLoadingResetTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 5_500_000_000) // 5.5s
            if Task.isCancelled { return }
            guard let self else { return }
            self.isRelayLoading = false
        }
    }

    private func tearDownRelaySearch() {
        if let handle = activeSearchHandle {
            let bridge = eventBridge
            let core = safeCore
            Task {
                await core.unsubscribe(handle)
                bridge?.unregister(handle: handle)
            }
            activeSearchHandle = nil
        }
        activeRelayQuery = ""
        relayLoadingResetTask?.cancel()
        relayLoadingResetTask = nil
    }

    /// EventBridge callback: the relay search delivered new matching events
    /// into ndb. Re-run the local article scan to pick them up. Guarded by
    /// query string so a late delta for a stale query doesn't clobber fresh
    /// results.
    func applyRelaySearchUpdate(query incomingQuery: String) {
        guard incomingQuery == appliedQuery, !appliedQuery.isEmpty else { return }
        isRelayLoading = false
        relayLoadingResetTask?.cancel()
        relayLoadingResetTask = nil
        let q = appliedQuery
        let token = searchToken
        Task { [weak self] in
            guard let self else { return }
            let refreshed = (try? await self.safeCore.searchArticles(query: q, limit: 30)) ?? []
            guard token == self.searchToken, q == self.appliedQuery else { return }
            self.articles = refreshed
        }
    }
}

