import Foundation
import Observation

/// View-scoped reactive state for a room's Discussions tab. Mirrors
/// `RoomStore.swift` — owns a per-view nostrdb read + subscription handle,
/// and applies `DiscussionUpserted` deltas routed by `EventBridge`.
@MainActor
@Observable
final class DiscussionStore {
    private(set) var discussions: [DiscussionRecord] = []
    private(set) var isLoading: Bool = true
    private(set) var loadError: String?

    @ObservationIgnored private var groupId: String?
    @ObservationIgnored private var core: SafeHighlighterCore?
    @ObservationIgnored private weak var bridge: EventBridge?
    @ObservationIgnored private var subscriptionHandle: UInt64?

    func start(groupId: String, core: SafeHighlighterCore, bridge: EventBridge?) async {
        self.groupId = groupId
        self.core = core
        self.bridge = bridge
        isLoading = true
        loadError = nil

        do {
            discussions = try await core.getDiscussions(groupId: groupId)
        } catch {
            loadError = (error as? CoreError).map { "\($0)" }
        }
        isLoading = false

        do {
            let handle = try await core.subscribeRoomDiscussions(groupId: groupId)
            subscriptionHandle = handle
            bridge?.registerDiscussions(self, handle: handle)
        } catch {
            // Subscription failure leaves cache-only rendering working.
        }
    }

    func stop() {
        if let handle = subscriptionHandle, let core {
            Task { await core.unsubscribe(handle) }
            bridge?.unregister(handle: handle)
        }
        subscriptionHandle = nil
    }

    func apply(discussion: DiscussionRecord) {
        if let i = discussions.firstIndex(where: { $0.eventId == discussion.eventId }) {
            discussions[i] = discussion
        } else {
            let merged = discussions + [discussion]
            discussions = merged.sorted { ($0.createdAt ?? 0) > ($1.createdAt ?? 0) }
        }
    }
}
