import Foundation

final class KernelHandle {
    private let raw: UnsafeMutableRawPointer
    private var updateSink: KernelUpdateSink?

    init() {
        raw = nmp_app_new()
    }

    deinit {
        nmp_app_set_update_callback(raw, nil, nil)
        nmp_app_free(raw)
    }

    func listen(_ handler: @escaping (KernelUpdateResult) -> Void) {
        let sink = KernelUpdateSink(handler: handler)
        updateSink = sink
        nmp_app_set_update_callback(raw, Unmanaged.passUnretained(sink).toOpaque(), nmpUpdateCallback)
    }

    func start(visibleLimit: UInt32, emitHz: UInt32) {
        nmp_app_start(raw, 0, visibleLimit, emitHz)
    }

    func configure(visibleLimit: UInt32, emitHz: UInt32) {
        nmp_app_configure(raw, 0, visibleLimit, emitHz)
    }

    func stop() {
        nmp_app_stop(raw)
    }

    func reset() {
        nmp_app_reset(raw)
    }

    func openAuthor(pubkey: String) {
        pubkey.withCString { nmp_app_open_author(raw, $0) }
    }

    func openThread(eventID: String) {
        eventID.withCString { nmp_app_open_thread(raw, $0) }
    }

    func openFirehose(tag: String) {
        tag.withCString { nmp_app_open_firehose_tag(raw, $0) }
    }

    func claimProfile(pubkey: String, consumerID: String) {
        pubkey.withCString { pubkeyPointer in
            consumerID.withCString { consumerPointer in
                nmp_app_claim_profile(raw, pubkeyPointer, consumerPointer)
            }
        }
    }

    func releaseProfile(pubkey: String, consumerID: String) {
        pubkey.withCString { pubkeyPointer in
            consumerID.withCString { consumerPointer in
                nmp_app_release_profile(raw, pubkeyPointer, consumerPointer)
            }
        }
    }

    func closeAuthor(pubkey: String) {
        pubkey.withCString { nmp_app_close_author(raw, $0) }
    }

    func closeThread(eventID: String) {
        eventID.withCString { nmp_app_close_thread(raw, $0) }
    }

    fileprivate static func decode(pointer: UnsafePointer<CChar>) -> KernelUpdateResult? {
        let start = ContinuousClock.now
        let payload = String(cString: pointer)
        let data = Data(payload.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        guard let update = try? decoder.decode(KernelUpdate.self, from: data) else {
            return nil
        }
        let duration = start.duration(to: .now)
        return KernelUpdateResult(
            update: update,
            payloadBytes: data.count,
            callbackReceivedAt: start,
            decodeMicros: duration.microseconds
        )
    }
}

private final class KernelUpdateSink {
    let handler: (KernelUpdateResult) -> Void

    init(handler: @escaping (KernelUpdateResult) -> Void) {
        self.handler = handler
    }
}

private let nmpUpdateCallback: NmpUpdateCallback = { context, pointer in
    guard let context, let pointer else {
        return
    }
    guard let result = KernelHandle.decode(pointer: pointer) else {
        return
    }
    let sink = Unmanaged<KernelUpdateSink>.fromOpaque(context).takeUnretainedValue()
    sink.handler(result)
}

struct KernelUpdateResult {
    let update: KernelUpdate
    let payloadBytes: Int
    let callbackReceivedAt: ContinuousClock.Instant
    let decodeMicros: Int
}

struct KernelUpdate: Decodable {
    let rev: UInt64
    let updateKind: String
    let running: Bool
    let relayUrl: String
    let testNpub: String
    let profile: ProfileCard
    let items: [TimelineItem]
    let authorView: AuthorViewPayload?
    let threadView: ThreadViewPayload?
    let inserted: [TimelineItem]
    let updated: [TimelineItem]
    let removed: [String]
    let metrics: KernelMetrics
    let relayStatus: RelayStatus
    let relayStatuses: [RelayStatus]
    let logicalInterests: [LogicalInterestStatus]
    let wireSubscriptions: [WireSubscriptionStatus]
    let logs: [String]
}

struct ProfileCard: Decodable, Equatable {
    let pubkey: String
    let npub: String
    let display: String
    let pictureUrl: String?
    let nip05: String
    let about: String
    let avatarInitials: String
    let avatarColor: String
    let source: String
}

struct TimelineItem: Decodable, Identifiable, Equatable {
    let id: String
    let authorPubkey: String
    let authorDisplay: String
    let authorPictureUrl: String?
    let authorAvatarInitials: String
    let authorAvatarColor: String
    let authorAvatarSource: String
    let content: String
    let contentPreview: String
    let createdAtDisplay: String
    let relayCount: UInt32
}

struct AuthorViewPayload: Decodable, Equatable {
    let pubkey: String
    let state: String
    let profile: ProfileCard
    let items: [TimelineItem]
    let noteCount: Int
}

struct ThreadViewPayload: Decodable, Equatable {
    let focusedEventId: String
    let rootEventId: String
    let state: String
    let items: [TimelineItem]
    let previousCount: Int
    let nextCount: Int
}

struct RelayStatus: Decodable, Equatable {
    let role: String
    let relayUrl: String
    let connection: String
    let auth: String
    let nip77Negentropy: String
    let activeWireSubscriptions: Int
    let reconnectCount: UInt32
    let lastConnectedAtMs: UInt64?
    let lastEventAtMs: UInt64?
    let lastNotice: String?
    let lastError: String?
    let bytesRx: UInt64
    let bytesTx: UInt64
}

struct WireSubscriptionStatus: Decodable, Identifiable, Equatable {
    var id: String { wireId }
    let wireId: String
    let relayUrl: String
    let filterSummary: String
    let state: String
    let logicalConsumerCount: UInt32
    let openedAtMs: UInt64
    let lastEventAtMs: UInt64?
    let eoseAtMs: UInt64?
    let closeReason: String?
}

struct LogicalInterestStatus: Decodable, Identifiable, Equatable {
    var id: String { key }
    let key: String
    let state: String
    let refcount: UInt32
    let relayUrls: [String]
    let cacheCoverage: String
    let warmingUntilMs: UInt64?
}

struct KernelMetrics: Decodable {
    let generatedEvents: UInt64
    let noteEvents: UInt64
    let profileEvents: UInt64
    let duplicateEvents: UInt64
    let deleteEvents: UInt64
    let storedEvents: Int
    let tombstones: Int
    let visibleItems: Int
    let visibleProfiledItems: Int
    let visiblePlaceholderAvatarItems: Int
    let openViews: UInt32
    let eventsSinceLastUpdate: UInt64
    let diagnosticFirehoseEvents: UInt64
    let insertedCount: Int
    let updatedCount: Int
    let removedCount: Int
    let eventsPerSecondConfigured: UInt32
    let emitHzConfigured: UInt32
    let updateSequence: UInt64
    let estimatedStoreBytes: Int
    let payloadBytes: Int
    let storeToPayloadRatio: Double
    let actorQueueDepth: UInt32
    let framesRx: UInt64
    let eventsRx: UInt64
    let eoseRx: UInt64
    let noticesRx: UInt64
    let closedRx: UInt64
    let bytesRx: UInt64
    let bytesTx: UInt64
    let contactsAuthors: Int
    let timelineAuthors: Int
    let firstEventMs: UInt64?
    let targetProfileLoadedMs: UInt64?
    let timelineOpenedMs: UInt64?
    let timelineFirstItemMs: UInt64?
    let updateEmittedMs: UInt64?
    let lastEventToEmitMs: UInt64?
    let maxEventToEmitMs: UInt64
    let maxEventsPerUpdate: UInt64
}

extension Duration {
    var microseconds: Int {
        let parts = components
        return Int(parts.seconds) * 1_000_000 + Int(parts.attoseconds / 1_000_000_000_000)
    }
}
