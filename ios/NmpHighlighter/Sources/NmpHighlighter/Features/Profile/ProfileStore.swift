import Foundation
import Observation

/// View-scoped store for a single profile page. Lifetime matches the
/// `ProfileView` that owns it — created in `onAppear`, torn down in
/// `onDisappear`. Subscribes via `subscribe_user_profile` so live deltas
/// (kind:0 / 3 / 30023 / 9802 / 39001 / 39002) trigger re-queries.
@MainActor
@Observable
final class ProfileStore {
    enum Tab: Hashable {
        case articles, highlights, communities
    }

    // Reactive state
    var profile: ProfileMetadata?
    var articles: [ArticleRecord] = []
    var highlights: [HighlightRecord] = []
    var communities: [CommunitySummary] = []
    var isFollowing: Bool = false
    var isMutatingFollow: Bool = false
    var followError: String?
    var isLoadingInitial: Bool = true
    var activeTab: Tab = .articles

    // Plumbing
    @ObservationIgnored let pubkey: String
    @ObservationIgnored let viewerPubkey: String?
    @ObservationIgnored let safeCore: SafeHighlighterCore
    @ObservationIgnored weak var eventBridge: EventBridge?
    @ObservationIgnored private var subscriptionHandle: UInt64?

    var isOwnProfile: Bool {
        guard let viewerPubkey else { return false }
        return viewerPubkey.lowercased() == pubkey.lowercased()
    }

    init(
        pubkey: String,
        viewerPubkey: String?,
        safeCore: SafeHighlighterCore,
        eventBridge: EventBridge?
    ) {
        self.pubkey = pubkey
        self.viewerPubkey = viewerPubkey
        self.safeCore = safeCore
        self.eventBridge = eventBridge
    }

    /// One-shot setup called from `ProfileView.task`. Kicks off the initial
    /// parallel loads, installs the subscription, and routes live deltas.
    func start() async {
        await loadAll()
        isLoadingInitial = false
        await installSubscription()
    }

    /// Called from `ProfileView.onDisappear`.
    func stop() {
        if let handle = subscriptionHandle {
            Task { [safeCore] in await safeCore.unsubscribe(handle) }
            eventBridge?.unregister(handle: handle)
            subscriptionHandle = nil
        }
    }

    // MARK: - Loads

    func loadAll() async {
        async let profileTask: ProfileMetadata? = {
            try? await safeCore.getUserProfile(pubkeyHex: pubkey)
        }()
        async let articlesTask: [ArticleRecord] = {
            (try? await safeCore.getUserArticles(pubkeyHex: pubkey)) ?? []
        }()
        async let highlightsTask: [HighlightRecord] = {
            (try? await safeCore.getUserHighlights(pubkeyHex: pubkey)) ?? []
        }()
        async let communitiesTask: [CommunitySummary] = {
            (try? await safeCore.getUserCommunities(pubkeyHex: pubkey)) ?? []
        }()
        async let followTask: Bool = {
            guard let viewer = viewerPubkey, viewer.lowercased() != pubkey.lowercased() else {
                return false
            }
            return (try? await safeCore.isFollowing(targetPubkeyHex: pubkey)) ?? false
        }()

        let (profile, articles, highlights, communities, following) = await (
            profileTask, articlesTask, highlightsTask, communitiesTask, followTask
        )
        self.profile = profile ?? self.profile
        self.articles = articles
        self.highlights = highlights
        self.communities = communities
        self.isFollowing = following
    }

    /// Called by `EventBridge` when a `UserProfileUpdated` delta arrives.
    /// Re-queries only the slice affected by the event kind.
    func applyUpdate(kind: UInt32) async {
        switch kind {
        case 0:
            if let p = try? await safeCore.getUserProfile(pubkeyHex: pubkey) {
                self.profile = p
            }
        case 3:
            // A contact list changed. If it's ours, refresh isFollowing.
            if let viewer = viewerPubkey,
               viewer.lowercased() != pubkey.lowercased() {
                if let b = try? await safeCore.isFollowing(targetPubkeyHex: pubkey) {
                    self.isFollowing = b
                }
            }
        case 30023:
            if let list = try? await safeCore.getUserArticles(pubkeyHex: pubkey) {
                self.articles = list
            }
        case 9802:
            if let list = try? await safeCore.getUserHighlights(pubkeyHex: pubkey) {
                self.highlights = list
            }
        case 39001, 39002:
            if let list = try? await safeCore.getUserCommunities(pubkeyHex: pubkey) {
                self.communities = list
            }
        default:
            break
        }
    }

    // MARK: - Follow

    func toggleFollow() async {
        guard let viewer = viewerPubkey, viewer.lowercased() != pubkey.lowercased() else {
            return
        }
        guard !isMutatingFollow else { return }
        isMutatingFollow = true
        followError = nil
        let wasFollowing = isFollowing
        isFollowing = !wasFollowing
        do {
            _ = try await safeCore.setFollow(
                targetPubkeyHex: pubkey,
                follow: !wasFollowing
            )
        } catch {
            isFollowing = wasFollowing
            followError = error.localizedDescription
        }
        isMutatingFollow = false
    }

    // MARK: - Private

    private func installSubscription() async {
        guard subscriptionHandle == nil, let bridge = eventBridge else { return }
        do {
            let handle = try await safeCore.subscribeUserProfile(pubkeyHex: pubkey)
            subscriptionHandle = handle
            bridge.registerProfile(self, handle: handle)
        } catch {
            // Non-fatal: the profile view still has its initial load. Live
            // updates will simply not stream in until the next visit.
        }
    }
}
