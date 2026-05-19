import Foundation
import Observation

/// View-scoped store backing the open-thread chat view. Loads every kind:1
/// `e`-tagged to the root (regardless of author so agent replies appear),
/// then receives per-event upserts from the bridge.
@MainActor
@Observable
final class FeedbackThreadStore {
    private(set) var events: [FeedbackEventRecord] = []
    private(set) var isLoading: Bool = true
    private(set) var loadError: String?
    private(set) var isPublishing: Bool = false

    @ObservationIgnored private var rootEventId: String?
    @ObservationIgnored private var coordinate: String?
    @ObservationIgnored private var agentPubkey: String?
    @ObservationIgnored private var core: SafeHighlighterCore?
    @ObservationIgnored private weak var bridge: EventBridge?
    @ObservationIgnored private var subscriptionHandle: UInt64?

    func start(
        rootEventId: String,
        coordinate: String,
        agentPubkey: String?,
        core: SafeHighlighterCore,
        bridge: EventBridge?
    ) async {
        self.rootEventId = rootEventId
        self.coordinate = coordinate
        self.agentPubkey = agentPubkey
        self.core = core
        self.bridge = bridge
        isLoading = true
        loadError = nil

        do {
            events = try await core.getFeedbackThreadEvents(rootEventId: rootEventId)
        } catch {
            loadError = (error as? CoreError).map { "\($0)" } ?? "\(error)"
        }
        isLoading = false

        do {
            let handle = try await core.subscribeFeedbackThread(rootEventId: rootEventId)
            subscriptionHandle = handle
            bridge?.registerFeedbackThread(self, handle: handle)
        } catch {
            // Cache-only rendering still works.
        }
    }

    func stop() {
        if let handle = subscriptionHandle, let core {
            Task { await core.unsubscribe(handle) }
            bridge?.unregister(handle: handle)
        }
        subscriptionHandle = nil
    }

    func apply(event: FeedbackEventRecord) {
        if let i = events.firstIndex(where: { $0.eventId == event.eventId }) {
            events[i] = event
        } else {
            events = (events + [event]).sorted { $0.createdAt < $1.createdAt }
        }
    }

    /// Send a reply into the open thread. Resolves the agent pubkey lazily
    /// if not already cached; publishes without a `p` tag when the project
    /// event isn't available.
    @discardableResult
    func sendReply(body: String) async throws -> FeedbackEventRecord {
        guard let core, let coordinate, let rootEventId else {
            throw FeedbackError.notReady
        }
        let trimmed = body.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            throw FeedbackError.notReady
        }
        var agent = agentPubkey
        if agent == nil {
            agent = try? await core.getProjectFirstAgentPubkey(coordinate: coordinate)
            agentPubkey = agent
        }

        isPublishing = true
        defer { isPublishing = false }

        let record = try await core.publishFeedbackNote(
            coordinate: coordinate,
            agentPubkey: agent,
            parentEventId: rootEventId,
            body: trimmed
        )
        apply(event: record)
        return record
    }
}
