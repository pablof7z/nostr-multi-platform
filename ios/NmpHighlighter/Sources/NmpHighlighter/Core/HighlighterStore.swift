import Foundation
import Observation

/// App-scoped reactive state. Only holds data that's genuinely global:
/// the current user, the set of joined communities (used by the tab root
/// and by the Capture flow's community picker), and connection health.
///
/// **Per-view data — a room's feed — does NOT live here.** Each view owns
/// a dedicated `@Observable` store (e.g. `RoomStore`) whose lifetime
/// matches the view. That keeps
/// Swift Observation granular and keeps the architectural contract that
/// nostrdb is the only source of truth: any data Swift shows must have
/// been read from (or written to) nostrdb first.
@MainActor
@Observable
final class HighlighterStore {
    // Reactive — drives UI
    var podcastPlayer = PodcastPlayerStore()
    var currentUser: CurrentUser?
    var currentUserProfile: ProfileMetadata?
    var joinedCommunities: [CommunitySummary] = [] {
        didSet { mirrorCommunitiesToAppGroup() }
    }
    var connectionState: ConnectionState = .unknown
    var isBootstrapping: Bool = false
    /// Transient toast shown when the Share Extension handoff publishes, a
    /// join request is sent, or a membership is confirmed. Cleared by the
    /// banner after a few seconds.
    var shareToast: String?
    /// Group ids for which the user has published a NIP-29 kind:9021 join
    /// request this session, mapped to the room name shown in the
    /// confirmation toast. When the next `MembershipChanged` delta for one
    /// of these arrives, the toast flips from "Join requested" to
    /// "You're in ✓" and the id drops from the map.
    @ObservationIgnored private var pendingJoins: [String: String] = [:]
    /// Shared profile cache — keyed by pubkey hex. Reactive so all card views
    /// observing a given pubkey re-render automatically when a fresh kind:0
    /// arrives from a relay.
    var profileCache: [String: ProfileMetadata] = [:]
    /// OpenGraph + favicon cache for web URL highlights, keyed by the
    /// canonical URL the metadata was fetched for. Mirrors `profileCache`'s
    /// shape so card views can look up enrichment synchronously and
    /// re-render when a fetch lands. The Rust core owns the on-disk cache;
    /// this dictionary is the in-memory mirror SwiftUI observes.
    var webMetadataCache: [String: WebMetadata] = [:]
    /// ArtifactPreview cache for ISBN lookups, keyed by bare ISBN-13 (e.g. "9780593716717").
    var isbnPreviewCache: [String: ArtifactPreview] = [:]
    /// NIP-51 kind:10003 article bookmarks — set of `30023:<pubkey>:<d>`
    /// addresses. Reactive so every row showing a bookmark affordance updates
    /// when the user toggles one anywhere.
    var bookmarkedArticleAddresses: Set<String> = []

    // Internal plumbing
    @ObservationIgnored let core: HighlighterCore
    @ObservationIgnored let safeCore: SafeHighlighterCore
    @ObservationIgnored private(set) var eventBridge: EventBridge?
    @ObservationIgnored private var joinedCommunitiesHandle: UInt64?
    @ObservationIgnored private var bookmarksHandle: UInt64?
    @ObservationIgnored private var profileCacheHandles: [String: UInt64] = [:]
    /// In-flight `requestWebMetadata` calls coalesce here so multiple rows
    /// referencing the same URL share a single Task. Cleared once the
    /// fetch completes (success or failure).
    @ObservationIgnored private var webMetadataInflight: [String: Task<Void, Never>] = [:]
    @ObservationIgnored private var isbnInflight: [String: Task<Void, Never>] = [:]

    var isLoggedIn: Bool { currentUser != nil }

    enum ConnectionState {
        case unknown, connecting, online, offline
    }

    init() {
        let core = HighlighterCore()
        self.core = core
        self.safeCore = SafeHighlighterCore(core: core)
        // Surface the MiniPlayer (paused) with whatever episode the user was
        // last listening to, if any. Tapping play wires AVPlayer through the
        // normal `load(artifact:)` path which seeks to the saved position.
        podcastPlayer.rehydrateFromSavedRecord()
    }

    func bootstrap() async {
        guard !isBootstrapping else { return }
        isBootstrapping = true
        defer { isBootstrapping = false }

        // Register the EventBridge unconditionally, before any login attempt.
        // The NIP-46 nostrconnect:// flow fires `SignerConnected` from a
        // background tokio task; if no callback is wired by then, the delta
        // is dropped silently and the UI never transitions to logged-in.
        registerEventBridge()

        if let user = await AppSessionStore.shared.restoreSession(into: safeCore) {
            currentUser = user
            await loadAppScopeData()
        }
    }

    func completeLogin(user: CurrentUser) async {
        currentUser = user
        if eventBridge == nil {
            registerEventBridge()
        }
        await loadAppScopeData()
    }

    func logout() {
        if let handle = joinedCommunitiesHandle {
            core.unsubscribe(handle: handle)
            eventBridge?.unregister(handle: handle)
            joinedCommunitiesHandle = nil
        }
        if let handle = bookmarksHandle {
            core.unsubscribe(handle: handle)
            eventBridge?.unregister(handle: handle)
            bookmarksHandle = nil
        }
        for (_, handle) in profileCacheHandles {
            core.unsubscribe(handle: handle)
            eventBridge?.unregister(handle: handle)
        }
        profileCacheHandles.removeAll()
        profileCache.removeAll()
        for (_, task) in webMetadataInflight { task.cancel() }
        webMetadataInflight.removeAll()
        webMetadataCache.removeAll()
        bookmarkedArticleAddresses.removeAll()
        core.logout()
        eventBridge = nil
        AppSessionStore.shared.clear()
        UserDefaults.standard.removeObject(forKey: "onboardingComplete")
        currentUser = nil
        currentUserProfile = nil
        joinedCommunities.removeAll()
        connectionState = .unknown
        SharedCommunitiesCache.clear()
    }

    // MARK: - Bookmarks

    /// Optimistic toggle: flip local state immediately for snappy UI, then
    /// publish. The inevitable `BookmarksUpdated` delta (ours or from another
    /// client) reconciles to authoritative state via `refreshBookmarks`.
    func toggleBookmark(articleAddress: String) async {
        let trimmed = articleAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        // Optimistic flip.
        if bookmarkedArticleAddresses.contains(trimmed) {
            bookmarkedArticleAddresses.remove(trimmed)
        } else {
            bookmarkedArticleAddresses.insert(trimmed)
        }
        // Authoritative toggle + publish.
        do {
            _ = try await safeCore.toggleArticleBookmark(address: trimmed)
            // No explicit refresh — the pump will deliver `BookmarksUpdated`.
        } catch {
            // Revert on failure.
            await refreshBookmarks()
        }
    }

    func refreshBookmarks() async {
        if let addrs = try? await safeCore.getBookmarkedArticleAddresses() {
            bookmarkedArticleAddresses = Set(addrs)
        }
    }

    func isBookmarked(articleAddress: String) -> Bool {
        bookmarkedArticleAddresses.contains(articleAddress)
    }

    /// Fetches a profile from the local nostrdb cache (fast path) and sets up
    /// a relay subscription so the cache is updated when a fresh kind:0 arrives.
    /// Safe to call from multiple views for the same pubkey — deduplicates.
    func requestProfile(pubkeyHex: String) async {
        if profileCache[pubkeyHex] == nil,
           let profile = try? await safeCore.getUserProfile(pubkeyHex: pubkeyHex) {
            profileCache[pubkeyHex] = profile
        }
        guard profileCacheHandles[pubkeyHex] == nil else { return }
        if let handle = try? await safeCore.subscribeUserProfile(pubkeyHex: pubkeyHex) {
            profileCacheHandles[pubkeyHex] = handle
            eventBridge?.registerProfileCache(pubkeyHex: pubkeyHex, handle: handle)
        }
    }

    /// Called by `EventBridge` when a subscribed profile's kind:0 arrives from a relay.
    func applyProfileCacheUpdate(pubkeyHex: String) async {
        if let profile = try? await safeCore.getUserProfile(pubkeyHex: pubkeyHex) {
            profileCache[pubkeyHex] = profile
        }
    }

    /// Fetch OpenGraph + favicon metadata for a web URL via the Rust core
    /// (which owns the disk cache + in-flight coalescing). Safe to call from
    /// multiple views for the same URL — the in-memory `webMetadataInflight`
    /// map deduplicates Swift-side, the Rust store deduplicates HTTP-side.
    /// No-op when the URL is already cached in `webMetadataCache`.
    func requestWebMetadata(url: String) async {
        let trimmed = url.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        if webMetadataCache[trimmed] != nil { return }
        if let existing = webMetadataInflight[trimmed] {
            await existing.value
            return
        }
        let task = Task { [weak self] in
            guard let self else { return }
            let metadata = try? await self.safeCore.getWebMetadata(url: trimmed)
            await MainActor.run {
                if let metadata {
                    self.webMetadataCache[trimmed] = metadata
                }
                self.webMetadataInflight.removeValue(forKey: trimmed)
            }
        }
        webMetadataInflight[trimmed] = task
        await task.value
    }

    /// Fetch + cache an ISBN preview. Concurrent callers for the same ISBN
    /// coalesce onto one in-flight Task. No-op when already cached.
    /// `isbn` must be the bare 13-digit string (no "isbn:" prefix).
    func requestIsbnPreview(isbn: String) async {
        let key = isbn.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !key.isEmpty else { return }
        if isbnPreviewCache[key] != nil { return }
        if let existing = isbnInflight[key] {
            await existing.value
            return
        }
        let task = Task { [weak self] in
            guard let self else { return }
            let preview = try? await self.safeCore.lookupIsbn(key)
            await MainActor.run {
                if let preview {
                    self.isbnPreviewCache[key] = preview
                }
                self.isbnInflight.removeValue(forKey: key)
            }
        }
        isbnInflight[key] = task
        await task.value
    }

    /// Snapshot `joinedCommunities` into the App Group cache so the Share
    /// Extension can render its community picker without loading the Rust
    /// core. Cheap — a JSON encode + UserDefaults set.
    private func mirrorCommunitiesToAppGroup() {
        let snapshot = joinedCommunities.map {
            SharedCommunitySummary(id: $0.id, name: $0.name, picture: $0.picture)
        }
        SharedCommunitiesCache.save(snapshot)
    }

    // MARK: - Private

    private func registerEventBridge() {
        let bridge = EventBridge(appStore: self)
        core.setEventCallback(callback: bridge)
        eventBridge = bridge
    }

    /// Public so `EventBridge` can re-query on a `MembershipChanged` delta.
    func refreshJoinedCommunities() async {
        if let updated = try? await safeCore.getJoinedCommunities() {
            joinedCommunities = updated
            // Any pending join whose group is now in the joined set →
            // promote the toast from "Join requested" to "You're in ✓".
            if !pendingJoins.isEmpty {
                let joinedIds = Set(updated.map(\.id))
                let confirmed = pendingJoins.filter { joinedIds.contains($0.key) }
                for (groupId, roomName) in confirmed {
                    pendingJoins.removeValue(forKey: groupId)
                    shareToast = "You're in \(roomName) ✓"
                }
            }
        }
    }

    /// Mark a join request as in-flight. Pops the "Join requested" toast
    /// immediately; the follow-up "You're in ✓" fires from
    /// `refreshJoinedCommunities` once a matching `MembershipChanged`
    /// delta lands.
    func noteJoinRequested(groupId: String, roomName: String) {
        let trimmedId = groupId.trimmingCharacters(in: .whitespaces)
        guard !trimmedId.isEmpty else { return }
        let cleanName = roomName.isEmpty ? "this room" : roomName
        pendingJoins[trimmedId] = cleanName
        shareToast = "Join requested"
    }

    private func loadAppScopeData() async {
        // Immediate read from nostrdb via the Rust core. Non-blocking on
        // relays — the cache answers first, subscriptions catch up later.
        if let cached = try? await safeCore.getJoinedCommunities() {
            joinedCommunities = cached
        }

        // Fetch the user's own kind:0 so the top-bar avatar shows their real
        // picture. Cheap — single nostrdb read. Lives on the app-scope store
        // because multiple surfaces (toolbar + future editors) need it.
        if let user = currentUser,
           let profile = try? await safeCore.getUserProfile(pubkeyHex: user.pubkey) {
            currentUserProfile = profile
        }

        // Publish the default Blossom server list if the user has never set one.
        // No-op when a kind:10063 is already cached. Fire-and-forget.
        try? await safeCore.initDefaultBlossomServers()

        // Install the joined-communities pump so future 39000/39001/39002
        // events apply to the app-scope store as CommunityUpserted /
        // MembershipChanged deltas (subscription_id == new handle, routed
        // by EventBridge).
        if joinedCommunitiesHandle == nil {
            if let handle = try? await safeCore.subscribeJoinedCommunities() {
                joinedCommunitiesHandle = handle
                // Joined-communities deltas are dispatched via the appStore
                // path in EventBridge (not per-view). No store registration
                // needed; we only hold the handle so logout can unsubscribe.
            }
        }

        // Hydrate the bookmark set from nostrdb, then install a live sub so
        // later kind:10003 events (ours or another client's) trigger a
        // `BookmarksUpdated` delta that refreshes the set.
        await refreshBookmarks()
        if bookmarksHandle == nil {
            if let handle = try? await safeCore.subscribeBookmarks() {
                bookmarksHandle = handle
            }
        }
    }
}
