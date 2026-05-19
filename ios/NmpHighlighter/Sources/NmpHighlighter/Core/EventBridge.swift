import Foundation
import os

/// Routes `Delta` notifications from Rust into the appropriate Swift
/// `@Observable` store.
///
/// Architecture: **nostrdb is source of truth.** The Rust core writes
/// every event to nostrdb, then emits `DataChangeType` deltas wrapped in
/// a `Delta` carrying the `subscription_id` that installed the pump.
/// `0` is reserved for app-scope deltas (signer state, joined-communities
/// summary). Any non-zero id routes to the view-scoped store that asked
/// for the subscription via `registerRoom` / `registerDiscussions`.
final class EventBridge: EventCallback, @unchecked Sendable {
    private weak var appStore: HighlighterStore?

    /// Weak registry of view-scoped stores keyed by subscription handle.
    /// Weak so a View deallocating automatically drops its store from the
    /// registry. Uses `OSAllocatedUnfairLock` (iOS 16+) so the lock is
    /// async-safe — `withLock { ... }` doesn't trip Swift 6's strict
    /// concurrency checks the way `NSLock` does.
    /// `@unchecked Sendable` is sound because every access goes through
    /// `OSAllocatedUnfairLock.withLock`, which serializes mutations. The
    /// `WeakBox` values hold weak references to `@MainActor`-isolated
    /// stores, so even if a reference survives into the wrong isolation
    /// it's nil or eventually nil'd by ARC.
    fileprivate struct Registry: @unchecked Sendable {
        var rooms: [UInt64: WeakBox<RoomStore>] = [:]
        var discussions: [UInt64: WeakBox<DiscussionStore>] = [:]
        var chats: [UInt64: WeakBox<ChatStore>] = [:]
        var chatPresence: [UInt64: WeakBox<ChatPresenceProbe>] = [:]
        var profiles: [UInt64: WeakBox<ProfileStore>] = [:]
        var articles: [UInt64: WeakBox<ArticleReaderStore>] = [:]
        var reads: [UInt64: WeakBox<ReadsStore>] = [:]
        var highlights: [UInt64: WeakBox<HighlightsStore>] = [:]
        var feedbackThreads: [UInt64: WeakBox<FeedbackStore>] = [:]
        var feedbackThreadDetails: [UInt64: WeakBox<FeedbackThreadStore>] = [:]
        var searches: [UInt64: WeakBox<SearchStore>] = [:]
        var bookmarks: [UInt64: WeakBox<BookmarkStore>] = [:]
        /// App-scoped Network Settings store (subscription_id == 0). Weak
        /// so it goes away when the screen is dismissed.
        var networkStore: WeakBox<NetworkSettingsStore>? = nil
        /// Explorer store — notified on CommunityUpserted so new rooms appear
        /// without requiring a pull-to-refresh.
        var explorerStore: WeakBox<RoomExplorerStore>? = nil
        /// Maps subscription handles → pubkey for app-scoped profile cache subscriptions.
        var profileCacheHandles: [UInt64: String] = [:]

        mutating func prune() {
            rooms = rooms.filter { $0.value.value != nil }
            discussions = discussions.filter { $0.value.value != nil }
            chats = chats.filter { $0.value.value != nil }
            chatPresence = chatPresence.filter { $0.value.value != nil }
            profiles = profiles.filter { $0.value.value != nil }
            articles = articles.filter { $0.value.value != nil }
            reads = reads.filter { $0.value.value != nil }
            highlights = highlights.filter { $0.value.value != nil }
            feedbackThreads = feedbackThreads.filter { $0.value.value != nil }
            feedbackThreadDetails = feedbackThreadDetails.filter { $0.value.value != nil }
            searches = searches.filter { $0.value.value != nil }
            bookmarks = bookmarks.filter { $0.value.value != nil }
        }
    }
    private let registry = OSAllocatedUnfairLock(initialState: Registry())

    init(appStore: HighlighterStore) {
        self.appStore = appStore
    }

    // MARK: - Registration (called by view stores when they subscribe)

    func registerRoom(_ store: RoomStore, handle: UInt64) {
        registry.withLock { reg in
            reg.rooms[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerDiscussions(_ store: DiscussionStore, handle: UInt64) {
        registry.withLock { reg in
            reg.discussions[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerChat(_ store: ChatStore, handle: UInt64) {
        registry.withLock { reg in
            reg.chats[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerChatPresence(_ probe: ChatPresenceProbe, handle: UInt64) {
        registry.withLock { reg in
            reg.chatPresence[handle] = WeakBox(probe)
            reg.prune()
        }
    }

    func registerProfile(_ store: ProfileStore, handle: UInt64) {
        registry.withLock { reg in
            reg.profiles[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerArticle(_ store: ArticleReaderStore, handle: UInt64) {
        registry.withLock { reg in
            reg.articles[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerReads(_ store: ReadsStore, handle: UInt64) {
        registry.withLock { reg in
            reg.reads[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerHighlights(_ store: HighlightsStore, handle: UInt64) {
        registry.withLock { reg in
            reg.highlights[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerFeedbackThreads(_ store: FeedbackStore, handle: UInt64) {
        registry.withLock { reg in
            reg.feedbackThreads[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerFeedbackThread(_ store: FeedbackThreadStore, handle: UInt64) {
        registry.withLock { reg in
            reg.feedbackThreadDetails[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerSearch(_ store: SearchStore, handle: UInt64) {
        registry.withLock { reg in
            reg.searches[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerBookmarkStore(_ store: BookmarkStore, handle: UInt64) {
        registry.withLock { reg in
            reg.bookmarks[handle] = WeakBox(store)
            reg.prune()
        }
    }

    func registerProfileCache(pubkeyHex: String, handle: UInt64) {
        registry.withLock { reg in
            reg.profileCacheHandles[handle] = pubkeyHex
            reg.prune()
        }
    }

    func registerNetworkStore(_ store: NetworkSettingsStore) {
        registry.withLock { reg in
            reg.networkStore = WeakBox(store)
        }
    }

    func registerExplorer(_ store: RoomExplorerStore) {
        registry.withLock { reg in
            reg.explorerStore = WeakBox(store)
        }
    }

    func unregister(handle: UInt64) {
        registry.withLock { reg in
            _ = reg.rooms.removeValue(forKey: handle)
            _ = reg.discussions.removeValue(forKey: handle)
            _ = reg.chats.removeValue(forKey: handle)
            _ = reg.chatPresence.removeValue(forKey: handle)
            _ = reg.profiles.removeValue(forKey: handle)
            _ = reg.articles.removeValue(forKey: handle)
            _ = reg.reads.removeValue(forKey: handle)
            _ = reg.highlights.removeValue(forKey: handle)
            _ = reg.feedbackThreads.removeValue(forKey: handle)
            _ = reg.feedbackThreadDetails.removeValue(forKey: handle)
            _ = reg.searches.removeValue(forKey: handle)
            _ = reg.bookmarks.removeValue(forKey: handle)
            _ = reg.profileCacheHandles.removeValue(forKey: handle)
        }
    }

    // MARK: - EventCallback

    func onDataChanged(delta: Delta) {
        Task { @MainActor in
            let change = delta.change
            let id = delta.subscriptionId

            if id == 0 {
                self.dispatchAppScope(change)
                return
            }

            let routed = self.registry.withLock { reg -> RoutedStores in
                RoutedStores(
                    room: reg.rooms[id]?.value,
                    discussion: reg.discussions[id]?.value,
                    chat: reg.chats[id]?.value,
                    chatPresence: reg.chatPresence[id]?.value,
                    profile: reg.profiles[id]?.value,
                    article: reg.articles[id]?.value,
                    reads: reg.reads[id]?.value,
                    highlights: reg.highlights[id]?.value,
                    feedback: reg.feedbackThreads[id]?.value,
                    feedbackThread: reg.feedbackThreadDetails[id]?.value,
                    search: reg.searches[id]?.value,
                    bookmark: reg.bookmarks[id]?.value,
                    profileCachePubkey: reg.profileCacheHandles[id]
                )
            }

            if let store = routed.room {
                self.dispatchRoom(change, store: store)
            } else if let store = routed.discussion {
                self.dispatchDiscussions(change, store: store)
            } else if let store = routed.chat {
                self.dispatchChat(change, store: store)
            } else if let probe = routed.chatPresence {
                self.dispatchChatPresence(change, probe: probe)
            } else if let store = routed.profile {
                self.dispatchProfile(change, store: store)
            } else if let store = routed.article {
                self.dispatchArticle(change, store: store)
            } else if let store = routed.reads {
                self.dispatchReads(change, store: store)
            } else if let store = routed.highlights {
                self.dispatchHighlights(change, store: store)
            } else if let store = routed.feedback {
                self.dispatchFeedbackThreads(change, store: store)
            } else if let store = routed.feedbackThread {
                self.dispatchFeedbackThread(change, store: store)
            } else if let store = routed.search {
                self.dispatchSearch(change, store: store)
            } else if let store = routed.bookmark {
                self.dispatchBookmarkStore(change, store: store)
            } else if let pubkey = routed.profileCachePubkey {
                self.dispatchProfileCache(change, pubkey: pubkey)
            }
        }
    }

    /// Snapshot of every view-scoped store that *might* own this delta's
    /// subscription handle. Routing is first-non-nil-wins; a handle is only
    /// ever registered to one store at a time.
    private struct RoutedStores {
        let room: RoomStore?
        let discussion: DiscussionStore?
        let chat: ChatStore?
        let chatPresence: ChatPresenceProbe?
        let profile: ProfileStore?
        let article: ArticleReaderStore?
        let reads: ReadsStore?
        let highlights: HighlightsStore?
        let feedback: FeedbackStore?
        let feedbackThread: FeedbackThreadStore?
        let search: SearchStore?
        let bookmark: BookmarkStore?
        let profileCachePubkey: String?
    }

    @MainActor
    private func dispatchFeedbackThreads(_ change: DataChangeType, store: FeedbackStore) {
        if case .feedbackThreadsUpdated = change {
            Task { await store.refreshThreads() }
        }
    }

    @MainActor
    private func dispatchFeedbackThread(_ change: DataChangeType, store: FeedbackThreadStore) {
        if case .feedbackThreadEventUpserted(let event) = change {
            store.apply(event: event)
        }
    }

    @MainActor
    private func dispatchArticle(_ change: DataChangeType, store: ArticleReaderStore) {
        if case .articleUpdated(_, let kind) = change {
            Task { await store.applyUpdate(kind: kind) }
        }
    }

    @MainActor
    private func dispatchProfile(_ change: DataChangeType, store: ProfileStore) {
        if case .userProfileUpdated(_, let kind) = change {
            Task { await store.applyUpdate(kind: kind) }
        }
    }

    @MainActor
    private func dispatchProfileCache(_ change: DataChangeType, pubkey: String) {
        guard case .userProfileUpdated(_, 0) = change else { return }
        if let appStore { Task { await appStore.applyProfileCacheUpdate(pubkeyHex: pubkey) } }
    }

    @MainActor
    private func dispatchAppScope(_ change: DataChangeType) {
        switch change {
        case .signerConnected(let user):
            if let appStore { Task { await appStore.completeLogin(user: user) } }
        case .relayStatusChanged(let url, let state):
            let store = registry.withLock { reg in reg.networkStore?.value }
            store?.applyStatus(url: url, state: state)
        case .communityUpserted, .membershipChanged:
            // Any group-related event arrived — re-query nostrdb for the
            // authoritative joined set. A single refresh path eliminates the
            // race where incremental upserts (CommunityUpserted) and
            // full-replace refreshes (MembershipChanged) contradicted each
            // other. The query is now membership-driven so missing metadata
            // never wipes the list.
            if let appStore { Task { await appStore.refreshJoinedCommunities() } }
            // Also notify the explorer so newly-discovered rooms appear
            // without requiring a pull-to-refresh.
            let explorer = registry.withLock { reg in reg.explorerStore?.value }
            if let explorer { Task { await explorer.reloadFromCache() } }
        case .bookmarksUpdated:
            if let appStore { Task { await appStore.refreshBookmarks() } }
        case .bunkerSignRequest:
            break
        default:
            break
        }
    }

    @MainActor
    private func dispatchRoom(_ change: DataChangeType, store: RoomStore) {
        switch change {
        case .artifactUpserted(_, let artifact):
            store.apply(artifact: artifact)
        case .highlightUpserted(_, let highlight):
            store.apply(highlight: highlight)
        case .highlightShared:
            // Kind:16 arrives as a hint that a new highlight belongs in the
            // room; the corresponding `highlightUpserted` (once the 9802 is
            // fetched) carries the body we display. No-op here.
            break
        default:
            break
        }
    }

    @MainActor
    private func dispatchDiscussions(_ change: DataChangeType, store: DiscussionStore) {
        switch change {
        case .discussionUpserted(_, let discussion):
            store.apply(discussion: discussion)
        default:
            break
        }
    }

    @MainActor
    private func dispatchChat(_ change: DataChangeType, store: ChatStore) {
        switch change {
        case .chatMessageUpserted(_, let message):
            store.apply(message: message)
        default:
            break
        }
    }

    @MainActor
    private func dispatchChatPresence(_ change: DataChangeType, probe: ChatPresenceProbe) {
        if case .chatMessageUpserted = change {
            probe.notifyActivity()
        }
    }

    @MainActor
    private func dispatchReads(_ change: DataChangeType, store: ReadsStore) {
        if case .followingReadsUpdated = change {
            Task { await store.refresh() }
        }
    }

    @MainActor
    private func dispatchHighlights(_ change: DataChangeType, store: HighlightsStore) {
        if case .followingHighlightsUpdated = change {
            Task { await store.refresh() }
        }
    }

    @MainActor
    private func dispatchSearch(_ change: DataChangeType, store: SearchStore) {
        if case .searchArticlesUpdated(let query) = change {
            store.applyRelaySearchUpdate(query: query)
        }
    }

    @MainActor
    private func dispatchBookmarkStore(_ change: DataChangeType, store: BookmarkStore) {
        switch change {
        case .bookmarkSetsUpdated, .followingCurationSetsUpdated, .webBookmarksUpdated:
            Task { await store.reload() }
        default:
            break
        }
    }

}

fileprivate final class WeakBox<T: AnyObject> {
    weak var value: T?
    init(_ value: T) { self.value = value }
}
