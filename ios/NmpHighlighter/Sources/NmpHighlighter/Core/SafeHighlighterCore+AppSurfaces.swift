import Foundation

extension SafeHighlighterCore {
    func buildPreviewFromUrl(_ url: String) async throws -> ArtifactPreview {
        try await core.buildPreviewFromUrl(url: url)
    }

    func getWebMetadata(url: String) async throws -> WebMetadata {
        try await core.getWebMetadata(url: url)
    }

    func getDiscussions(groupId: String, limit: UInt32 = 64) async throws -> [DiscussionRecord] {
        try await core.getDiscussions(groupId: groupId, limit: limit)
    }

    // MARK: - Chat (NIP-29 kind:9)

    func getChatMessages(groupId: String, limit: UInt32 = 200) async throws -> [ChatMessageRecord] {
        try await core.getChatMessages(groupId: groupId, limit: limit)
    }

    func publishChatMessage(
        groupId: String,
        content: String,
        replyToEventId: String? = nil
    ) async throws -> ChatMessageRecord {
        try await core.publishChatMessage(
            groupId: groupId,
            content: content,
            replyToEventId: replyToEventId
        )
    }

    func subscribeRoomChat(groupId: String) async throws -> UInt64 {
        try await core.subscribeRoomChat(groupId: groupId)
    }

    // MARK: - Feedback (shake-to-share)

    func getFeedbackThreads(coordinate: String) async throws -> [FeedbackThreadRecord] {
        try await core.getFeedbackThreads(coordinate: coordinate)
    }

    func getFeedbackThreadEvents(rootEventId: String) async throws -> [FeedbackEventRecord] {
        try await core.getFeedbackThreadEvents(rootEventId: rootEventId)
    }

    func getProjectFirstAgentPubkey(coordinate: String) async throws -> String? {
        try await core.getProjectFirstAgentPubkey(coordinate: coordinate)
    }

    func publishFeedbackNote(
        coordinate: String,
        agentPubkey: String?,
        parentEventId: String?,
        body: String
    ) async throws -> FeedbackEventRecord {
        try await core.publishFeedbackNote(
            coordinate: coordinate,
            agentPubkey: agentPubkey,
            parentEventId: parentEventId,
            body: body
        )
    }

    func subscribeFeedbackThreads(coordinate: String) async throws -> UInt64 {
        try await core.subscribeFeedbackThreads(coordinate: coordinate)
    }

    func subscribeFeedbackThread(rootEventId: String) async throws -> UInt64 {
        try await core.subscribeFeedbackThread(rootEventId: rootEventId)
    }

    // MARK: - Profile reads

    func getUserProfile(pubkeyHex: String) async throws -> ProfileMetadata? {
        try await core.getUserProfile(pubkeyHex: pubkeyHex)
    }

    func decodeNostrEntity(_ input: String) throws -> NostrEntityRef {
        try core.decodeNostrEntity(input: input)
    }

    /// Mint a NIP-19 `nevent` for an event id with optional author / kind / relay
    /// hints. Used to build shareable highlight URLs (e.g. for the
    /// `https://highlighter.com/highlight/<nevent>` social-card flow).
    func encodeNevent(
        eventIdHex: String,
        authorPubkeyHex: String?,
        relayHints: [String],
        kind: UInt32?
    ) throws -> String {
        try core.encodeEventToNevent(
            eventIdHex: eventIdHex,
            authorPubkeyHex: authorPubkeyHex,
            relayHints: relayHints,
            kind: kind
        )
    }

    func resolveNostrEntity(_ entity: NostrEntityRef) async throws -> NostrEntityEvent? {
        try await core.resolveNostrEntity(entity: entity)
    }

    func subscribeNostrEntity(_ entity: NostrEntityRef) async throws {
        try await core.subscribeNostrEntity(entity: entity)
    }

    func updateProfile(
        name: String,
        displayName: String,
        about: String,
        picture: String,
        banner: String,
        nip05: String,
        website: String,
        lud16: String
    ) async throws -> ProfileMetadata {
        try await core.updateProfile(
            name: name,
            displayName: displayName,
            about: about,
            picture: picture,
            banner: banner,
            nip05: nip05,
            website: website,
            lud16: lud16
        )
    }

    func getUserArticles(pubkeyHex: String, limit: UInt32 = 32) async throws -> [ArticleRecord] {
        try await core.getUserArticles(pubkeyHex: pubkeyHex, limit: limit)
    }

    func getArticle(pubkeyHex: String, dTag: String) async throws -> ArticleRecord? {
        try await core.getArticle(pubkeyHex: pubkeyHex, dTag: dTag)
    }

    func getHighlightsForArticle(address: String, limit: UInt32 = 128) async throws -> [HighlightRecord] {
        try await core.getHighlightsForArticle(address: address, limit: limit)
    }

    func getHighlightsForReference(
        tagName: String,
        tagValue: String,
        limit: UInt32 = 128
    ) async throws -> [HighlightRecord] {
        try await core.getHighlightsForReference(tagName: tagName, tagValue: tagValue, limit: limit)
    }

    func getCommentsForReference(
        tagName: String,
        tagValue: String,
        limit: UInt32 = 128
    ) async throws -> [CommentRecord] {
        try await core.getCommentsForReference(tagName: tagName, tagValue: tagValue, limit: limit)
    }

    func publishComment(
        rootTagName: String,
        rootTagValue: String,
        rootKind: UInt16,
        parentEventId: String? = nil,
        content: String
    ) async throws -> CommentRecord {
        try await core.publishComment(rootTagName: rootTagName, rootTagValue: rootTagValue, rootKind: rootKind, parentEventId: parentEventId, content: content)
    }

    func getUserHighlights(pubkeyHex: String, limit: UInt32 = 64) async throws -> [HighlightRecord] {
        try await core.getUserHighlights(pubkeyHex: pubkeyHex, limit: limit)
    }

    func getUserCommunities(pubkeyHex: String) async throws -> [CommunitySummary] {
        try await core.getUserCommunities(pubkeyHex: pubkeyHex)
    }

    // MARK: - Rooms explorer

    func startRoomDiscovery() async {
        await core.startRoomDiscovery()
    }

    func startFriendsRoomsDiscovery() async throws {
        try await core.startFriendsRoomsDiscovery()
    }

    func startFeaturedRooms(curatorPubkeyHex: String) async throws {
        try await core.startFeaturedRooms(curatorPubkeyHex: curatorPubkeyHex)
    }

    func getFeaturedRooms(curatorPubkeyHex: String) async throws -> [CommunitySummary] {
        try await core.getFeaturedRooms(curatorPubkeyHex: curatorPubkeyHex).filter(\.isPublicOpenRoom)
    }

    func getAllRooms(limit: UInt32 = 120) async throws -> [CommunitySummary] {
        let candidates = try await core.getAllRooms(limit: publicRoomCandidateLimit(limit))
        return Array(candidates.filter(\.isPublicOpenRoom).prefix(Int(limit)))
    }

    func getNewRooms(limit: UInt32 = 24) async throws -> [CommunitySummary] {
        let candidates = try await core.getNewRooms(limit: publicRoomCandidateLimit(limit))
        return Array(candidates.filter(\.isPublicOpenRoom).prefix(Int(limit)))
    }

    func getRoomsWithFriends(limit: UInt32 = 16) async throws -> [RoomRecommendation] {
        let candidates = try await core.getRoomsWithFriends(limit: publicRoomCandidateLimit(limit))
        return Array(candidates.filter { $0.summary.isPublicOpenRoom }.prefix(Int(limit)))
    }

    func getRoomsFromReadAuthors(limit: UInt32 = 16) async throws -> [RoomRecommendation] {
        let candidates = try await core.getRoomsFromReadAuthors(limit: publicRoomCandidateLimit(limit))
        return Array(candidates.filter { $0.summary.isPublicOpenRoom }.prefix(Int(limit)))
    }

    func requestJoinRoom(groupId: String) async throws -> String {
        try await core.requestJoinRoom(groupId: groupId)
    }

    func createRoom(
        name: String,
        about: String,
        picture: String,
        visibility: RoomVisibility,
        access: RoomAccess
    ) async throws -> String {
        try await core.createRoom(
            name: name,
            about: about,
            picture: picture,
            visibility: visibility,
            access: access
        )
    }

    func addRoomMember(groupId: String, pubkeyHex: String) async throws -> String {
        try await core.addRoomMember(groupId: groupId, pubkeyHex: pubkeyHex)
    }

    func createRoomInviteCodes(groupId: String, count: UInt32) async throws -> [String] {
        try await core.createRoomInviteCodes(groupId: groupId, count: count)
    }

    func getFollows() async throws -> [String] {
        try await core.getFollows()
    }

    func decodeNpub(_ input: String) throws -> String {
        try core.decodeNpub(input: input)
    }

    func isFollowing(targetPubkeyHex: String) async throws -> Bool {
        try await core.isFollowing(targetPubkeyHex: targetPubkeyHex)
    }

    func setFollow(targetPubkeyHex: String, follow: Bool) async throws -> String? {
        try await core.setFollow(targetPubkeyHex: targetPubkeyHex, follow: follow)
    }

    // MARK: - Following Reads

    func getFollowingReads(limit: UInt32 = 40) async throws -> [ReadingFeedItem] {
        try await core.getFollowingReads(limit: limit)
    }

    // MARK: - Following Highlights

    func getFollowingHighlights(limit: UInt32 = 120) async throws -> [HydratedHighlight] {
        try await core.getFollowingHighlights(limit: limit)
    }

    // MARK: - Subscriptions

    func subscribeFollowingReads() async throws -> UInt64 {
        try await core.subscribeFollowingReads()
    }

    func subscribeFollowingHighlights() async throws -> UInt64 {
        try await core.subscribeFollowingHighlights()
    }

    func subscribeJoinedCommunities() async throws -> UInt64 {
        try await core.subscribeJoinedCommunities()
    }

    func subscribeRoom(groupId: String) async throws -> UInt64 {
        try await core.subscribeRoom(groupId: groupId)
    }

    func subscribeRoomDiscussions(groupId: String) async throws -> UInt64 {
        try await core.subscribeRoomDiscussions(groupId: groupId)
    }

    func subscribeUserProfile(pubkeyHex: String) async throws -> UInt64 {
        try await core.subscribeUserProfile(pubkeyHex: pubkeyHex)
    }

    func subscribeArticle(pubkeyHex: String, dTag: String) async throws -> UInt64 {
        try await core.subscribeArticle(pubkeyHex: pubkeyHex, dTag: dTag)
    }

    func unsubscribe(_ handle: UInt64) {
        core.unsubscribe(handle: handle)
    }

    // MARK: - Writes

    func publishArtifact(
        preview: ArtifactPreview,
        groupId: String,
        note: String?
    ) async throws -> ArtifactRecord {
        try await core.publishArtifact(preview: preview, groupId: groupId, note: note)
    }

    func publishDiscussion(
        groupId: String,
        title: String,
        body: String,
        attachment: ArtifactPreview?
    ) async throws -> DiscussionRecord {
        try await core.publishDiscussion(
            groupId: groupId,
            title: title,
            body: body,
            attachment: attachment
        )
    }

    func publishHighlightsAndShare(
        artifact: ArtifactRecord,
        drafts: [HighlightDraft],
        targetGroupId: String
    ) async throws -> [HighlightRecord] {
        try await core.publishHighlightsAndShare(
            artifact: artifact,
            drafts: drafts,
            targetGroupId: targetGroupId
        )
    }

    func publishHighlight(
        draft: HighlightDraft,
        artifact: ArtifactRecord
    ) async throws -> HighlightRecord {
        try await core.publishHighlight(draft: draft, artifact: artifact)
    }

    /// Re-share an existing highlight into a room as a kind:16 repost.
    /// `relayHint` may be empty — the core falls back to the Highlighter
    /// relay for the e-tag hint when so.
    func shareHighlightToRoom(
        highlightId: String,
        highlightAuthorPubkeyHex: String,
        highlightRelayUrl: String,
        targetGroupId: String
    ) async throws {
        try await core.shareHighlightToRoom(
            highlightId: highlightId,
            highlightAuthorPubkeyHex: highlightAuthorPubkeyHex,
            highlightRelayUrl: highlightRelayUrl,
            targetGroupId: targetGroupId
        )
    }

    // MARK: - Blossom (BUD-03, kind:10063)

    func getBlossomServers() async throws -> [String] {
        try await core.getBlossomServers()
    }

    func setBlossomServers(_ servers: [String]) async throws -> String {
        try await core.setBlossomServers(servers: servers)
    }

    func initDefaultBlossomServers() async throws {
        try await core.initDefaultBlossomServers()
    }

    func signNip98Auth(url: String, method: String, payloadHash: String?) async throws -> String {
        try await core.signNip98Auth(url: url, method: method, payloadHash: payloadHash)
    }

    func signNip05RegistrationAuth(name: String, domain: String) async throws -> String {
        try await core.signNip05RegistrationAuth(name: name, domain: domain)
    }

    // MARK: - Capture (Blossom upload + kind:20 picture)

    func uploadPhoto(
        bytes: Data,
        mime: String,
        width: UInt32,
        height: UInt32,
        alt: String
    ) async throws -> BlossomUpload {
        try await core.uploadPhoto(
            bytes: bytes,
            mime: mime,
            width: width,
            height: height,
            alt: alt
        )
    }

    func publishPicture(_ draft: PictureDraft) async throws -> PictureRecord {
        try await core.publishPicture(draft: draft)
    }

    // MARK: - Relay config (NIP-65 read/write + NIP-78 rooms/indexer)

    func getRelays() async throws -> [RelayConfig] {
        try await core.getRelays()
    }

    func upsertRelay(_ cfg: RelayConfig) async throws {
        try await core.upsertRelay(cfg: cfg)
    }

    func removeRelay(_ url: String) async throws {
        try await core.removeRelay(url: url)
    }

    func setRelayRoles(
        url: String,
        read: Bool,
        write: Bool,
        rooms: Bool,
        indexer: Bool
    ) async throws {
        try await core.setRelayRoles(
            url: url,
            read: read,
            write: write,
            rooms: rooms,
            indexer: indexer
        )
    }

    // MARK: - Relay telemetry (PR 4)

    func getRelayDiagnostics() async throws -> [RelayDiagnostic] {
        try await core.getRelayDiagnostics()
    }

    func subscribeRelayStatus() async throws -> UInt64 {
        try await core.subscribeRelayStatus()
    }

    func reconnectAll() async throws {
        try await core.reconnectAll()
    }

    func disconnectAll() async throws {
        try await core.disconnectAll()
    }

    func probeRelayNip11(_ url: String) async throws -> Nip11Document {
        try await core.probeRelayNip11(url: url)
    }

    func importRelaysFromNpub(_ npub: String) async throws -> [RelayConfig] {
        try await core.importRelaysFromNpub(npub: npub)
    }

    func getCacheStats() async throws -> CacheStats {
        try await core.getCacheStats()
    }

    func publicRoomCandidateLimit(_ limit: UInt32) -> UInt32 {
        let expanded = limit > UInt32.max / 4 ? UInt32.max : limit * 4
        return max(limit, min(expanded, 512))
    }
}

