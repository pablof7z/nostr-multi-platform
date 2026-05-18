import Foundation
import Observation

/// View-scoped store for the Following Reads feed. Owned by `HomeFeedStore`
/// — lifetime matches the Highlights home tab (start in `.task`, tear down
/// in `.onDisappear`).
///
/// Two sources of truth:
/// - **nostrdb** (via Rust core): the feed is rebuilt from the local cache
///   on every `FollowingReadsUpdated` delta.
/// - **Relay subscriptions**: installed by `subscribeFollowingReads`, which
///   opens two relay subs (direct articles by follows + interactions on
///   kind:30023) and fires `FollowingReadsUpdated` deltas as events land.
@MainActor
@Observable
final class ReadsStore {
    var items: [ReadingFeedItem] = []
    var isLoadingInitial: Bool = true
    var loadError: String?

    @ObservationIgnored let safeCore: SafeHighlighterCore
    @ObservationIgnored weak var eventBridge: EventBridge?
    @ObservationIgnored private var subscriptionHandle: UInt64?

    init(safeCore: SafeHighlighterCore, eventBridge: EventBridge?) {
        self.safeCore = safeCore
        self.eventBridge = eventBridge
    }

    func start() async {
        await refresh()
        isLoadingInitial = false
        await installSubscription()
    }

    func stop() {
        guard let handle = subscriptionHandle else { return }
        Task { [safeCore] in await safeCore.unsubscribe(handle) }
        eventBridge?.unregister(handle: handle)
        subscriptionHandle = nil
    }

    func refresh() async {
        if let updated = try? await safeCore.getFollowingReads() {
            items = updated
        }
    }

    private func installSubscription() async {
        guard subscriptionHandle == nil, let bridge = eventBridge else { return }
        guard let handle = try? await safeCore.subscribeFollowingReads() else { return }
        subscriptionHandle = handle
        bridge.registerReads(self, handle: handle)
    }
}
