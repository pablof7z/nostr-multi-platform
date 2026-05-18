import Foundation

/// Thin C-FFI wrapper around the `nmp_core` static library. Mirrors
/// `ios/NmpStress/NmpStress/KernelBridge.swift` (which is the established
/// pattern). Pulse currently consumes only the timeline-reading surface; the
/// publish / sign-in / multi-account FFI is filed as T66a (see
/// `ios/NmpPulse/README.md`).
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

    func listen(_ handler: @escaping (KernelUpdate) -> Void) {
        let sink = KernelUpdateSink(handler: handler)
        updateSink = sink
        nmp_app_set_update_callback(raw, Unmanaged.passUnretained(sink).toOpaque(), nmpUpdateCallback)
    }

    func start(visibleLimit: UInt32 = 80, emitHz: UInt32 = 4) {
        nmp_app_start(raw, 0, visibleLimit, emitHz)
    }

    func stop() {
        nmp_app_stop(raw)
    }

    func openAuthor(pubkey: String) {
        pubkey.withCString { nmp_app_open_author(raw, $0) }
    }

    func openThread(eventID: String) {
        eventID.withCString { nmp_app_open_thread(raw, $0) }
    }

    func claimProfile(pubkey: String, consumerID: String) {
        pubkey.withCString { pkPtr in
            consumerID.withCString { cidPtr in
                nmp_app_claim_profile(raw, pkPtr, cidPtr)
            }
        }
    }

    func releaseProfile(pubkey: String, consumerID: String) {
        pubkey.withCString { pkPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_profile(raw, pkPtr, cidPtr)
            }
        }
    }

    fileprivate static func decode(pointer: UnsafePointer<CChar>) -> KernelUpdate? {
        let payload = String(cString: pointer)
        let data = Data(payload.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try? decoder.decode(KernelUpdate.self, from: data)
    }
}

private final class KernelUpdateSink {
    let handler: (KernelUpdate) -> Void
    init(handler: @escaping (KernelUpdate) -> Void) {
        self.handler = handler
    }
}

private let nmpUpdateCallback: NmpUpdateCallback = { context, pointer in
    guard let context, let pointer else { return }
    guard let update = KernelHandle.decode(pointer: pointer) else { return }
    let sink = Unmanaged<KernelUpdateSink>.fromOpaque(context).takeUnretainedValue()
    sink.handler(update)
}

// ─── Decoded snapshot shape ────────────────────────────────────────────────
//
// Pulse uses a tighter subset of the NmpStress KernelUpdate. The kernel
// emits ALL these fields; Pulse just doesn't reference the metrics-heavy
// ones. If the kernel ever stops emitting one, JSON decode fails closed
// (the @Published model stays at its prior value) — no field is required.

struct KernelUpdate: Decodable {
    let rev: UInt64
    let running: Bool
    let relayUrl: String
    let testNpub: String
    let profile: ProfileCard
    let items: [TimelineItem]
    let metrics: KernelMetricsLite
    let relayStatuses: [RelayStatus]
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
    let content: String
    let contentPreview: String
    let createdAtDisplay: String
    let relayCount: UInt32
}

struct KernelMetricsLite: Decodable {
    let storedEvents: Int
    let visibleItems: Int
    let eventsRx: UInt64
    let updateSequence: UInt64
}

struct RelayStatus: Decodable, Equatable, Identifiable {
    var id: String { relayUrl }
    let role: String
    let relayUrl: String
    let connection: String
    let auth: String
    let activeWireSubscriptions: Int
    let reconnectCount: UInt32
}
