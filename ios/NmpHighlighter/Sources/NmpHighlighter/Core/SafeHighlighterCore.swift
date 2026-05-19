import Foundation

/// Actor-isolated wrapper around the UniFFI-generated `HighlighterCore` so
/// Swift call sites get a clean `async throws` API without worrying about
/// FFI thread safety. Mirrors TENEX's `SafeTenexCore`.
actor SafeHighlighterCore {
    let core: HighlighterCore

    // Persistent ISBN metadata cache — avoids re-fetching Open Library for
    // books the user has already scanned, even across app launches.
    private struct CachedISBNPreview: Codable {
        var id: String
        var url: String
        var title: String
        var author: String
        var image: String
        var description: String
        var domain: String
        var publishedAt: String
    }
    private static let isbnCacheKey = "app.highlighter.isbn_cache_v1"
    private var isbnCache: [String: CachedISBNPreview] = [:]
    private var isbnCacheLoaded = false

    init(core: HighlighterCore) {
        self.core = core
    }

    // MARK: - Auth

    func loginNsec(_ nsec: String) throws -> CurrentUser {
        try core.loginNsec(nsec: nsec)
    }

    func startNostrConnect(_ options: NostrConnectOptions) async throws -> String {
        try await core.startNostrConnect(options: options)
    }

    func pairBunker(_ uri: String) async throws -> CurrentUser {
        try await core.pairBunker(uri: uri)
    }

    func generateAccount() throws -> GeneratedAccount {
        try core.generateAccount()
    }

    func currentUser() -> CurrentUser? {
        core.currentUser()
    }

    // MARK: - Reads

    func getJoinedCommunities() async throws -> [CommunitySummary] {
        try await core.getJoinedCommunities()
    }

    func getArtifacts(groupId: String, limit: UInt32 = 32) async throws -> [ArtifactRecord] {
        try await core.getArtifacts(groupId: groupId, limit: limit)
    }

    func getHighlights(groupId: String, limit: UInt32 = 64) async throws -> [HydratedHighlight] {
        try await core.getHighlights(groupId: groupId, limit: limit)
    }

    func getRecentBooks(limit: UInt32 = 24) async throws -> [ArtifactRecord] {
        try await core.getRecentBooks(limit: limit)
    }

    func searchArtifacts(query: String, limit: UInt32 = 20) async throws -> [ArtifactRecord] {
        try await core.searchArtifacts(query: query, limit: limit)
    }

    // MARK: - Search (local ndb + NIP-50 relay)

    func searchHighlights(query: String, limit: UInt32 = 20) async throws -> [HighlightRecord] {
        try await core.searchHighlights(query: query, limit: limit)
    }

    func searchArticles(query: String, limit: UInt32 = 20) async throws -> [ArticleRecord] {
        try await core.searchArticles(query: query, limit: limit)
    }

    func searchCommunities(query: String, limit: UInt32 = 20) async throws -> [CommunitySummary] {
        let candidates = try await core.searchCommunities(query: query, limit: publicRoomCandidateLimit(limit))
        return Array(candidates.filter(\.isPublicOpenRoom).prefix(Int(limit)))
    }

    func searchProfiles(query: String, limit: UInt32 = 20) async throws -> [ProfileMetadata] {
        try await core.searchProfiles(query: query, limit: limit)
    }

    func getSearchRelays() async throws -> [String] {
        try await core.getSearchRelays()
    }

    func subscribeArticleSearch(query: String) async throws -> UInt64 {
        try await core.subscribeArticleSearch(query: query)
    }

    // MARK: - Bookmarks (NIP-51 kind:10003)

    func getBookmarkedArticleAddresses() async throws -> [String] {
        try await core.getBookmarkedArticleAddresses()
    }

    func isArticleBookmarked(address: String) async throws -> Bool {
        try await core.isArticleBookmarked(address: address)
    }

    func toggleArticleBookmark(address: String) async throws -> Bool {
        try await core.toggleArticleBookmark(address: address)
    }

    func subscribeBookmarks() async throws -> UInt64 {
        try await core.subscribeBookmarks()
    }

    // MARK: - Reactions (kind:7)

    func getReactionsForEvent(targetEventId: String, limit: UInt32) async throws -> [ReactionRecord] {
        try await core.getReactionsForEvent(targetEventId: targetEventId, limit: limit)
    }

    func publishReaction(eventId: String, authorPubkeyHex: String, targetKind: UInt16, content: String) async throws -> ReactionRecord {
        try await core.publishReaction(eventId: eventId, authorPubkeyHex: authorPubkeyHex, targetKind: targetKind, content: content)
    }

    func unpublishReaction(reactionEventId: String) async throws -> String {
        try await core.unpublishReaction(reactionEventId: reactionEventId)
    }

    // MARK: - Event bookmarks (kind:10003 note bookmarks)

    func isEventBookmarked(eventIdHex: String) async throws -> Bool {
        try await core.isEventBookmarked(eventIdHex: eventIdHex)
    }

    func toggleEventBookmark(eventIdHex: String) async throws -> Bool {
        try await core.toggleEventBookmark(eventIdHex: eventIdHex)
    }

    // MARK: - Bookmark sets (kind:30003/30004) + NIP-B0 (kind:39701)

    func getMyBookmarkSets() async throws -> [BookmarkSetRecord] {
        try await core.getMyBookmarkSets()
    }

    func getMyCurationSets() async throws -> [BookmarkSetRecord] {
        try await core.getMyCurationSets()
    }

    func getFollowingCurationSets() async throws -> [BookmarkSetRecord] {
        try await core.getFollowingCurationSets()
    }

    func createCurationSet(title: String) async throws -> BookmarkSetRecord {
        try await core.createCurationSet(title: title)
    }

    @discardableResult
    func setAddressInCurationSet(
        dTag: String,
        address: String,
        member: Bool
    ) async throws -> Bool {
        try await core.setAddressInCurationSet(dTag: dTag, address: address, member: member)
    }

    func getMyWebBookmarks() async throws -> [WebBookmarkRecord] {
        try await core.getMyWebBookmarks()
    }

    func subscribeBookmarkSets() async throws -> UInt64 {
        try await core.subscribeBookmarkSets()
    }

    func subscribeFollowingCurationSets() async throws -> UInt64 {
        try await core.subscribeFollowingCurationSets()
    }

    func subscribeWebBookmarks() async throws -> UInt64 {
        try await core.subscribeWebBookmarks()
    }

    func lookupIsbn(_ isbn: String) async throws -> ArtifactPreview {
        loadIsbnCacheIfNeeded()
        if let hit = isbnCache[isbn] {
            return makePreview(from: hit, isbn: isbn)
        }
        let preview = try await core.lookupIsbn(isbn: isbn)
        isbnCache[isbn] = CachedISBNPreview(
            id: preview.id,
            url: preview.url,
            title: preview.title,
            author: preview.author,
            image: preview.image,
            description: preview.description,
            domain: preview.domain,
            publishedAt: preview.publishedAt
        )
        persistIsbnCache()
        return preview
    }

    private func loadIsbnCacheIfNeeded() {
        guard !isbnCacheLoaded else { return }
        isbnCacheLoaded = true
        guard let data = UserDefaults.standard.data(forKey: Self.isbnCacheKey),
              let dict = try? JSONDecoder().decode([String: CachedISBNPreview].self, from: data)
        else { return }
        isbnCache = dict
    }

    private func persistIsbnCache() {
        guard let data = try? JSONEncoder().encode(isbnCache) else { return }
        UserDefaults.standard.set(data, forKey: Self.isbnCacheKey)
    }

    private func makePreview(from cached: CachedISBNPreview, isbn: String) -> ArtifactPreview {
        let catalogId = "isbn:\(isbn)"
        return ArtifactPreview(
            id: cached.id,
            url: cached.url,
            title: cached.title,
            author: cached.author,
            image: cached.image,
            description: cached.description,
            source: "book",
            domain: cached.domain,
            catalogId: catalogId,
            catalogKind: "isbn",
            podcastGuid: "",
            podcastItemGuid: "",
            podcastShowTitle: "",
            audioUrl: "",
            audioPreviewUrl: "",
            transcriptUrl: "",
            feedUrl: "",
            publishedAt: cached.publishedAt,
            durationSeconds: nil,
            referenceTagName: "i",
            referenceTagValue: catalogId,
            referenceKind: "isbn",
            highlightTagName: "i",
            highlightTagValue: catalogId,
            highlightReferenceKey: "i:\(catalogId)",
            chapters: []
        )
    }

}
