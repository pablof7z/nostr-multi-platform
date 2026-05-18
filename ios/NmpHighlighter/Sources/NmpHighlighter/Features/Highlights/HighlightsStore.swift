import Foundation
import Observation

/// View-scoped store for the Highlights home feed — kind:9802 events from
/// people the user follows plus highlights tagged into any room they've
/// joined. Lifetime matches `HighlightsTabView` (start in `.task`, tear down
/// in `.onDisappear`). The Rust core owns the query + subscription; on each
/// `FollowingHighlightsUpdated` delta we re-query and replace `items`.
@MainActor
@Observable
final class HighlightsStore {
    var items: [HydratedHighlight] = []
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
        do {
            let updated = try await safeCore.getFollowingHighlights(limit: 120)
            items = updated
            loadError = nil
        } catch {
            loadError = String(describing: error)
        }
    }

    private func installSubscription() async {
        guard subscriptionHandle == nil, let bridge = eventBridge else { return }
        guard let handle = try? await safeCore.subscribeFollowingHighlights() else { return }
        subscriptionHandle = handle
        bridge.registerHighlights(self, handle: handle)
    }
}
