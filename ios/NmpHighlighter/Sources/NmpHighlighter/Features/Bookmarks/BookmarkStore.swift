import Foundation
import Observation

enum BookmarkScope {
    case mine, explore
}

@MainActor
@Observable
final class BookmarkStore {
    // Mine-mode data
    var myArticles: [ArticleRecord] = []
    var myBookmarkSets: [BookmarkSetRecord] = []
    var myCurationSets: [BookmarkSetRecord] = []
    var myWebBookmarks: [WebBookmarkRecord] = []

    // Explore-mode data
    var followingCurationSets: [BookmarkSetRecord] = []

    var scope: BookmarkScope = .mine
    var isLoading = false

    private var setsHandle: UInt64?
    private var followingHandle: UInt64?
    private var webHandle: UInt64?

    private weak var bridge: EventBridge?
    private var core: SafeHighlighterCore?

    func start(addresses: Set<String>, core: SafeHighlighterCore, bridge: EventBridge) async {
        self.core = core
        self.bridge = bridge

        if let h = try? await core.subscribeBookmarkSets() {
            setsHandle = h
            bridge.registerBookmarkStore(self, handle: h)
        }
        if let h = try? await core.subscribeFollowingCurationSets() {
            followingHandle = h
            bridge.registerBookmarkStore(self, handle: h)
        }
        if let h = try? await core.subscribeWebBookmarks() {
            webHandle = h
            bridge.registerBookmarkStore(self, handle: h)
        }

        await withTaskGroup(of: Void.self) { group in
            group.addTask { await self.reload() }
            group.addTask { await self.loadArticles(addresses: addresses) }
        }
    }

    func stop() {
        if let h = setsHandle { bridge?.unregister(handle: h); setsHandle = nil }
        if let h = followingHandle { bridge?.unregister(handle: h); followingHandle = nil }
        if let h = webHandle { bridge?.unregister(handle: h); webHandle = nil }
    }

    func reload() async {
        guard let core else { return }
        isLoading = true
        defer { isLoading = false }

        async let sets = (try? await core.getMyBookmarkSets()) ?? []
        async let curations = (try? await core.getMyCurationSets()) ?? []
        async let webs = (try? await core.getMyWebBookmarks()) ?? []
        async let following = (try? await core.getFollowingCurationSets()) ?? []

        myBookmarkSets = await sets
        myCurationSets = await curations
        myWebBookmarks = await webs

        // Drop curations from Explore that would render as "Empty Collection"
        // — either zero items at all, or every articleAddress fails to resolve
        // against the local NostrDB cache and there are no note refs to
        // fall back on. Mine keeps empty sets so authors can edit drafts.
        let raw = await following
        followingCurationSets = await Self.dropEmpty(raw, core: core)
    }

    /// Returns the subset of `sets` whose detail view would actually render
    /// at least one item. Resolves articles against the local cache via
    /// `getArticle` (cheap NostrDB read, no relay round-trip); short-circuits
    /// per set on the first hit.
    private static func dropEmpty(
        _ sets: [BookmarkSetRecord],
        core: SafeHighlighterCore
    ) async -> [BookmarkSetRecord] {
        await withTaskGroup(of: (Int, Bool).self) { group in
            for (idx, set) in sets.enumerated() {
                group.addTask {
                    (idx, await hasResolvableItem(set, core: core))
                }
            }
            var keep = Set<Int>()
            for await (idx, ok) in group where ok {
                keep.insert(idx)
            }
            return sets.enumerated()
                .compactMap { keep.contains($0.offset) ? $0.element : nil }
        }
    }

    private static func hasResolvableItem(
        _ set: BookmarkSetRecord,
        core: SafeHighlighterCore
    ) async -> Bool {
        if !set.noteIds.isEmpty { return true }
        if set.articleAddresses.isEmpty { return false }
        for address in set.articleAddresses {
            let parts = address.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
            guard parts.count == 3, parts[0] == "30023" else { continue }
            let pubkey = String(parts[1])
            let dTag = String(parts[2])
            guard !pubkey.isEmpty, !dTag.isEmpty else { continue }
            if (try? await core.getArticle(pubkeyHex: pubkey, dTag: dTag)) != nil {
                return true
            }
        }
        return false
    }

    func loadArticles(addresses: Set<String>) async {
        guard let core, !addresses.isEmpty else {
            myArticles = []
            return
        }
        var loaded: [ArticleRecord] = []
        for address in addresses {
            let parts = address.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
            guard parts.count == 3, parts[0] == "30023" else { continue }
            let pubkey = String(parts[1])
            let dTag = String(parts[2])
            guard !pubkey.isEmpty, !dTag.isEmpty else { continue }
            if let article = try? await core.getArticle(pubkeyHex: pubkey, dTag: dTag) {
                loaded.append(article)
            }
        }
        myArticles = loaded.sorted {
            ($0.publishedAt ?? $0.createdAt ?? 0) > ($1.publishedAt ?? $1.createdAt ?? 0)
        }
    }
}
