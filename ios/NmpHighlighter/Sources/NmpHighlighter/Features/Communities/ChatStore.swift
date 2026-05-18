import Foundation
import Observation

/// Lightweight presence probe for "does this room have chat activity?".
/// Distinct from `ChatStore` because the answer drives whether the Chat
/// tab is even shown — we want to know without spinning up the full chat
/// list view first.
///
/// The probe peeks the cache (`getChatMessages(groupId, limit: 1)`) on
/// `start` and installs the same room-chat subscription as `ChatStore`
/// so that a freshly-arriving kind:9 unhides the tab live. Once activity
/// is signalled the probe stays subscribed (cheap) until `stop`.
@MainActor
@Observable
final class ChatPresenceProbe {
    @ObservationIgnored private var groupId: String?
    @ObservationIgnored private var core: SafeHighlighterCore?
    @ObservationIgnored private weak var bridge: EventBridge?
    @ObservationIgnored private var subscriptionHandle: UInt64?
    @ObservationIgnored private var onActivity: (() -> Void)?

    func start(
        groupId: String,
        core: SafeHighlighterCore,
        bridge: EventBridge?,
        onActivity: @escaping () -> Void
    ) async {
        self.groupId = groupId
        self.core = core
        self.bridge = bridge
        self.onActivity = onActivity

        // Cache peek first — instant if any kind:9 is already locally cached.
        if let messages = try? await core.getChatMessages(groupId: groupId, limit: 1),
           !messages.isEmpty {
            onActivity()
        }

        do {
            let handle = try await core.subscribeRoomChat(groupId: groupId)
            subscriptionHandle = handle
            bridge?.registerChatPresence(self, handle: handle)
        } catch {
            // No live promotion if the subscription failed; the cache peek
            // result still applies.
        }
    }

    func stop() {
        if let handle = subscriptionHandle, let core {
            Task { await core.unsubscribe(handle) }
            bridge?.unregister(handle: handle)
        }
        subscriptionHandle = nil
        onActivity = nil
    }

    /// Called by `EventBridge` for the first `ChatMessageUpserted` after
    /// `start`. Idempotent — repeat calls just re-fire the closure (harmless
    /// because the consumer flips a Bool to true).
    func notifyActivity() {
        onActivity?()
    }
}

/// View-scoped reactive state for a room's Chat tab. Mirrors
/// `DiscussionStore.swift` — owns a per-view nostrdb read + subscription
/// handle, and applies `ChatMessageUpserted` deltas routed by `EventBridge`.
///
/// Messages are kept in ascending `created_at` order so the chat view can
/// render newest-at-bottom without re-sorting on each apply.
@MainActor
@Observable
final class ChatStore {
    static let pageSize: UInt32 = 50

    private(set) var messages: [ChatMessageRecord] = []
    private(set) var isLoading: Bool = true
    private(set) var isLoadingMore: Bool = false
    /// True when the last page fetch returned a full page, implying older
    /// messages exist in the DB beyond the current window.
    private(set) var hasMore: Bool = false
    private(set) var loadError: String?
    var sendError: String?

    @ObservationIgnored private var groupId: String?
    @ObservationIgnored private var core: SafeHighlighterCore?
    @ObservationIgnored private weak var bridge: EventBridge?
    @ObservationIgnored private var subscriptionHandle: UInt64?
    @ObservationIgnored private var loadedLimit: UInt32 = ChatStore.pageSize

    func start(groupId: String, core: SafeHighlighterCore, bridge: EventBridge?) async {
        self.groupId = groupId
        self.core = core
        self.bridge = bridge
        loadedLimit = ChatStore.pageSize
        isLoading = true
        loadError = nil

        do {
            let batch = try await core.getChatMessages(groupId: groupId, limit: loadedLimit)
            messages = batch
            hasMore = UInt32(batch.count) >= loadedLimit
        } catch {
            loadError = (error as? CoreError).map { "\($0)" }
        }
        isLoading = false

        do {
            let handle = try await core.subscribeRoomChat(groupId: groupId)
            subscriptionHandle = handle
            bridge?.registerChat(self, handle: handle)
        } catch {
            // Subscription failure leaves cache-only rendering working.
        }
    }

    /// Expand the loaded window by one page. Replaces `messages` with a
    /// larger slice from the DB; the caller is responsible for restoring
    /// the scroll position to the previously-topmost visible message.
    func loadMore() async {
        guard !isLoadingMore, hasMore, let groupId, let core else { return }
        isLoadingMore = true
        let newLimit = loadedLimit + ChatStore.pageSize
        do {
            let batch = try await core.getChatMessages(groupId: groupId, limit: newLimit)
            messages = batch
            loadedLimit = newLimit
            hasMore = UInt32(batch.count) >= newLimit
        } catch {}
        isLoadingMore = false
    }

    func stop() {
        if let handle = subscriptionHandle, let core {
            Task { await core.unsubscribe(handle) }
            bridge?.unregister(handle: handle)
        }
        subscriptionHandle = nil
    }

    /// Called by `EventBridge` for each `ChatMessageUpserted` delta. Inserts
    /// or replaces by `eventId`; keeps the array sorted ascending so the
    /// view's reverse-stream renders newest-at-bottom for free.
    func apply(message: ChatMessageRecord) {
        if let i = messages.firstIndex(where: { $0.eventId == message.eventId }) {
            messages[i] = message
            return
        }
        // Most arrivals are newer than everything we have; cheap fast-path.
        if let last = messages.last, message.createdAt >= last.createdAt {
            messages.append(message)
            return
        }
        messages.append(message)
        messages.sort { $0.createdAt < $1.createdAt }
    }

    /// Send a chat message into the room. Network publish; the live
    /// subscription will deliver the relay echo as a `ChatMessageUpserted`
    /// delta which `apply(message:)` upserts (so we don't need to insert
    /// the returned record — the apply path is idempotent).
    func send(text: String, replyTo: ChatMessageRecord? = nil) async {
        guard let groupId, let core else { return }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        sendError = nil
        do {
            let record = try await core.publishChatMessage(
                groupId: groupId,
                content: trimmed,
                replyToEventId: replyTo?.eventId
            )
            // Optimistic insert in case the relay echo is slow — apply is
            // upsert by event id, so the eventual delta merges cleanly.
            apply(message: record)
        } catch {
            sendError = "\(error)"
        }
    }
}
