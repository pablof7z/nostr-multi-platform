import Darwin
import Foundation
import os.log

let kbLog = Logger(subsystem: "com.example.Chirp", category: "KernelBridge")

/// Thin C-FFI wrapper around the `nmp_core` static library.
final class KernelHandle {
    let raw: UnsafeMutableRawPointer
    private var updateSink: KernelUpdateSink?
    /// Strong reference to the registered capabilities object. Held so the
    /// context pointer passed to `nmpCapabilityCallback` stays valid until
    /// `deinit` unregisters the callback.
    private var retainedCapabilities: ChirpCapabilities?
    /// T146 — opaque handle returned by `nmp_app_chirp_register`. The
    /// modular-timeline bridge extension manages its lifetime; see
    /// `Bridge/ModularTimelineBridge.swift`.
    var chirpHandle: UnsafeMutableRawPointer?
    /// Opaque handle returned by `nmp_app_chirp_marmot_register`. The
    /// Marmot bridge extension manages its lifetime; see
    /// `Bridge/MarmotBridge.swift`. Registered lazily once a secret key is
    /// known (nsec sign-in); nil until then (and for bunker sign-in).
    var marmotHandle: UnsafeMutableRawPointer?

    init() {
        raw = nmp_app_new()
        // Stage 4 of NIP-46 wiring: initialise the bunker broker before any
        // `signInBunker(...)` dispatch can reach the actor. The broker
        // registers a hook with `nmp-core` that drives the NIP-46 connect /
        // get_public_key handshake on a worker thread, then ships the
        // resulting signer back via `AddRemoteSigner`. D0 stays clean — the
        // broker is a separate static lib (`libnmp_signer_broker.a`).
        nmp_signer_broker_init(raw)
        // T146 — register the modular timeline projection on the kernel
        // event observer slot. See `Bridge/ModularTimelineBridge.swift`.
        registerChirpProjection()
    }

    deinit {
        // T146 — drop the projection BEFORE `nmp_app_free` per FFI contract.
        unregisterChirpProjectionIfNeeded()
        // Same contract for the Marmot observer registration.
        unregisterMarmotIfNeeded()
        nmp_app_set_update_callback(raw, nil, nil)
        // Unregister the capability callback before releasing `retainedCapabilities`
        // so no callback fires with a dangling context pointer.
        nmp_app_set_capability_callback(raw, nil, nil)
        retainedCapabilities = nil
        nmp_app_free(raw)
    }

    /// Register the native keyring capability handler. The Rust kernel routes
    /// every keyring `CapabilityRequest` through this seam. Must be called
    /// before `start()` so the handler is in place for any capability requests
    /// the actor issues during startup.
    func registerCapabilityHandler(_ capabilities: ChirpCapabilities) {
        retainedCapabilities = capabilities
        nmp_app_set_capability_callback(
            raw,
            Unmanaged.passUnretained(capabilities).toOpaque(),
            nmpCapabilityCallback)
    }

    func listen(_ handler: @escaping (KernelUpdateResult) -> Void) {
        let sink = KernelUpdateSink(handler: handler)
        updateSink = sink
        nmp_app_set_update_callback(raw, Unmanaged.passUnretained(sink).toOpaque(), nmpUpdateCallback)
    }

    func start(visibleLimit: UInt32 = 80, emitHz: UInt32 = 4) {
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

    /// Signal that the author feed for `pubkey` is no longer visible.
    /// Tears down the author-subscription so the kernel's wire_subs count
    /// returns to baseline. Call from `.onDisappear` on the AuthorView
    /// (ProfileView) to prevent sub-leaks on navigation pop.
    func closeAuthor(pubkey: String) {
        pubkey.withCString { nmp_app_close_author(raw, $0) }
    }

    /// Signal that the thread for `eventID` is no longer visible.
    /// Symmetric counterpart to `openThread`; call from `.onDisappear`
    /// on the ThreadScreen to release the thread subscription.
    func closeThread(eventID: String) {
        eventID.withCString { nmp_app_close_thread(raw, $0) }
    }

    // ── T66a identity / publish / multi-account / relay-edit ──────────────

    func signInNsec(_ secret: String) {
        secret.withCString { nmp_app_signin_nsec(raw, $0) }
    }

    func signInBunker(_ uri: String) {
        uri.withCString { nmp_app_signin_bunker(raw, $0) }
    }

    /// Cancel an in-flight NIP-46 bunker handshake. Idempotent / safe when
    /// nothing is in flight (no-op).
    func cancelBunkerHandshake() {
        nmp_app_cancel_bunker_handshake(raw)
    }

    /// Generate a fresh `nostrconnect://` URI for the QR-code NIP-46 sign-in
    /// flow. Returns `nil` if the broker is not yet initialised (which would
    /// be unusual — it's init'd in `KernelHandle.init()`). Each call produces
    /// a new ephemeral keypair and session secret.
    func nostrConnectURI(relay: String) -> String? {
        relay.withCString { relayPtr in
            guard let ptr = nmp_app_nostrconnect_uri(raw, relayPtr) else { return nil }
            defer { nmp_broker_free_string(ptr) }
            return String(cString: ptr)
        }
    }

    func createAccount(profile: [String: String], relays: [(String, String)]) {
        let profileJson = try! JSONSerialization.data(withJSONObject: profile, options: [])
        let profileStr = String(data: profileJson, encoding: .utf8)!
        let relaysJson = try! JSONSerialization.data(withJSONObject: relays.map { [$0.0, $0.1] }, options: [])
        let relaysStr = String(data: relaysJson, encoding: .utf8)!
        profileStr.withCString { profilePtr in
            relaysStr.withCString { relaysPtr in
                nmp_app_create_new_account(raw, profilePtr, relaysPtr)
            }
        }
    }

    func publishProfile(profile: [String: String]) {
        let profileJson = try! JSONSerialization.data(withJSONObject: profile, options: [])
        let content = String(data: profileJson, encoding: .utf8)!
        let unsigned: [String: Any] = [
            "pubkey": "",
            "kind": 0,
            "tags": [],
            "content": content,
            "created_at": UInt64(Date().timeIntervalSince1970)
        ]
        let data = try! JSONSerialization.data(withJSONObject: unsigned, options: [])
        let json = String(data: data, encoding: .utf8)!
        json.withCString { nmp_app_publish_unsigned_event(raw, $0) }
    }

    func switchActive(identityID: String) {
        identityID.withCString { nmp_app_switch_active(raw, $0) }
    }

    func removeAccount(identityID: String) {
        identityID.withCString { nmp_app_remove_account(raw, $0) }
    }

    func publishNote(content: String, replyToID: String?) {
        content.withCString { cPtr in
            if let replyToID {
                replyToID.withCString { rPtr in
                    nmp_app_publish_note(raw, cPtr, rPtr)
                }
            } else {
                nmp_app_publish_note(raw, cPtr, nil)
            }
        }
    }

    func react(targetEventID: String, reaction: String) {
        targetEventID.withCString { tPtr in
            reaction.withCString { rPtr in
                nmp_app_react(raw, tPtr, rPtr)
            }
        }
    }

    func follow(pubkey: String) {
        pubkey.withCString { nmp_app_follow(raw, $0) }
    }

    func unfollow(pubkey: String) {
        pubkey.withCString { nmp_app_unfollow(raw, $0) }
    }

    func addRelay(url: String, role: String) {
        url.withCString { uPtr in
            role.withCString { rPtr in
                nmp_app_add_relay(raw, uPtr, rPtr)
            }
        }
    }

    func removeRelay(url: String) {
        url.withCString { nmp_app_remove_relay(raw, $0) }
    }

    func openTimeline() {
        nmp_app_open_timeline(raw)
    }

    // ── NIP-47 Wallet Connect ─────────────────────────────────────────────

    func walletConnect(uri: String) {
        uri.withCString { nmp_app_wallet_connect(raw, $0) }
    }

    func walletDisconnect() {
        nmp_app_wallet_disconnect(raw)
    }

    func walletPayInvoice(bolt11: String, amountMsats: UInt64?) {
        bolt11.withCString { bPtr in
            if let amount = amountMsats {
                let amountStr = String(amount)
                amountStr.withCString { aPtr in
                    nmp_app_wallet_pay_invoice(raw, bPtr, aPtr)
                }
            } else {
                nmp_app_wallet_pay_invoice(raw, bPtr, nil)
            }
        }
    }

    // ── T118 / G3 — iOS scenePhase → kernel lifecycle bridge ─────────────
    //
    // Chirp's `@main` App observes `@Environment(\.scenePhase)` and routes
    // `.active` / `.background` through here. The kernel decides what each
    // phase MEANS (D7): scenePhase reports the fact, the kernel reacts —
    // fans `TriggerEvent::Foreground` through its internal observer to
    // wake the NIP-77 reconciler on Background→Foreground.
    //
    // `.inactive` (the interstitial state between active and background) is
    // silently dropped at the call site — no Swift method, no FFI symbol;
    // the actor's lifecycle reducer never sees it.

    /// Report iOS scenePhase = `.active`. Idempotent: while the kernel is
    /// already foregrounded, this is a debounced no-op.
    func lifecycleForeground() {
        nmp_app_lifecycle_foreground(raw)
    }

    /// Report iOS scenePhase = `.background`. Idempotent.
    func lifecycleBackground() {
        nmp_app_lifecycle_background(raw)
    }

    fileprivate static func decode(pointer: UnsafePointer<CChar>) -> KernelUpdateResult? {
        let start = ContinuousClock.now
        let payload = String(cString: pointer)
        let data = Data(payload.utf8)
        guard let outer = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            // PD-025 finding 4: log the offending payload prefix so decode failures
            // are observable (the original regression: decode failure → hasActiveAccount
            // never flips → stuck on OnboardingView). Toast injection is impossible
            // here because the toast surface is driven by the snapshot that just failed
            // to decode — logging is the only correct path at this layer.
            let preview = payload.prefix(200)
            kbLog.error("outer JSON parse failed — payload prefix: \(preview)")
            return nil
        }
        let frameTag = outer["t"] as? String
        guard frameTag == "snapshot" else {
            // Discrete update frames (t=update) are intentionally ignored — the
            // snapshot already carries full projected UI state. Log at debug so
            // a flood of unhandled frame types is diagnosable without noise.
            if frameTag == "update" {
                kbLog.debug("discrete update frame received (not applied by snapshot bridge)")
            } else {
                kbLog.error("unknown envelope tag=\(frameTag ?? "<nil>") — payload prefix: \(payload.prefix(200))")
            }
            return nil
        }
        guard let inner = outer["v"] else {
            kbLog.error("snapshot missing 'v' field")
            return nil
        }
        guard let innerData = try? JSONSerialization.data(withJSONObject: inner) else {
            kbLog.error("failed to re-serialize inner JSON")
            return nil
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        do {
            let update = try decoder.decode(KernelUpdate.self, from: innerData)
            let duration = start.duration(to: .now)
            kbLog.info("decoded ok rev=\(update.rev) activeAccount=\(update.activeAccount ?? "nil")")
            return KernelUpdateResult(
                update: update,
                payloadBytes: data.count,
                callbackReceivedAt: start,
                decodeMicros: duration.microseconds
            )
        } catch {
            kbLog.error("decode error: \(error.localizedDescription)")
            if let preview = String(data: innerData.prefix(500), encoding: .utf8) {
                kbLog.error("JSON preview: \(preview)")
            }
            return nil
        }
    }
}

private final class KernelUpdateSink {
    let handler: (KernelUpdateResult) -> Void
    init(handler: @escaping (KernelUpdateResult) -> Void) {
        self.handler = handler
    }
}

/// C capability callback — receives `CapabilityRequest` JSON from Rust and
/// returns a malloc-allocated `CapabilityEnvelope` JSON string that Rust frees
/// via `nmp_app_free_string` / `CString::from_raw`. Uses `strdup` so the
/// allocation is compatible with Rust's `CString::from_raw` on Apple platforms
/// (both use the system malloc allocator).
private let nmpCapabilityCallback: NmpCapabilityCallback = { context, requestJSON in
    guard let context, let requestJSON else { return nil }
    let capabilities = Unmanaged<ChirpCapabilities>.fromOpaque(context).takeUnretainedValue()
    let requestStr = String(cString: requestJSON)
    let resultStr = capabilities.keyring.handleJSON(requestStr)
    return resultStr.withCString { strdup($0) }
}

private let nmpUpdateCallback: NmpUpdateCallback = { context, pointer in
    guard let context, let pointer else { return }
    let payload = String(cString: pointer)
    if payload.contains("\"t\":\"panic\"") {
        kbLog.fault("NMP_ACTOR_PANIC detected")
        NSLog("NMP_ACTOR_PANIC: %@", payload)
        return
    }
    guard let result = KernelHandle.decode(pointer: pointer) else { return }
    let sink = Unmanaged<KernelUpdateSink>.fromOpaque(context).takeUnretainedValue()
    sink.handler(result)
}

// ─── Swift-side timing wrapper ────────────────────────────────────────────

struct KernelUpdateResult {
    let update: KernelUpdate
    let payloadBytes: Int
    let callbackReceivedAt: ContinuousClock.Instant
    let decodeMicros: Int
}

// ─── Decoded snapshot shape ───────────────────────────────────────────────

struct KernelUpdate: Decodable {
    let rev: UInt64
    let updateKind: String?
    let running: Bool
    let relayUrl: String
    let testNpub: String
    let profile: ProfileCard
    let items: [TimelineItem]
    // Delta tracking — optional so older snapshots without these still decode.
    let inserted: [TimelineItem]?
    let updated: [TimelineItem]?
    let removed: [String]?
    let metrics: KernelMetrics
    // Single-relay backwards compat field alongside the array.
    let relayStatus: RelayStatus?
    let relayStatuses: [RelayStatus]
    // Perf diagnostics — optional so old kernels still decode (D1).
    let logicalInterests: [LogicalInterestStatus]?
    let wireSubscriptions: [WireSubscriptionStatus]?
    let logs: [String]?
    // T66a projections. Optional so a kernel that elides one (or an older
    // build) still decodes — the model keeps its prior value (D1).
    let threadView: ThreadView?
    let accounts: [AccountSummary]?
    let activeAccount: String?
    let publishQueue: [PublishQueueEntry]?
    let lastErrorToast: String?
    let relayEditRows: [RelayEditRow]?
    // NIP-47 wallet projection. Optional so older kernels still decode (D1).
    let walletStatus: WalletStatusData?
    // NIP-46 bunker handshake progress (Stage 3 backend emits this).
    // Optional so older kernels still decode (D1).
    let bunkerHandshake: BunkerHandshake?
}

/// NIP-46 (`bunker://`) handshake progress, projected from the kernel snapshot.
/// Stage values: `"connecting"`, `"awaiting_pubkey"`, `"ready"`, `"failed"`,
/// `"idle"`. `message` is a human-readable progress / error hint.
struct BunkerHandshake: Decodable, Equatable {
    let stage: String
    let message: String?
}

// ─── Perf-diagnostic types ────────────────────────────────────────────────

struct LogicalInterestStatus: Decodable, Identifiable, Equatable {
    var id: String { key }
    let key: String
    let state: String
    let refcount: UInt32
    let relayUrls: [String]
    let cacheCoverage: String
    let warmingUntilMs: UInt64?
}

struct WireSubscriptionStatus: Decodable, Identifiable, Equatable {
    var id: String { wireId }
    let wireId: String
    let relayUrl: String
    let filterSummary: String
    let state: String
    let logicalConsumerCount: UInt32
    let eventsRx: UInt64?
    let openedAtMs: UInt64
    let lastEventAtMs: UInt64?
    let eoseAtMs: UInt64?
    let closeReason: String?
}

// ─── Domain types shared across the UI ───────────────────────────────────

struct ThreadView: Decodable, Equatable {
    let focusedEventId: String
    let rootEventId: String
    let state: String
    let items: [TimelineItem]
    let previousCount: Int
    let nextCount: Int
}

struct AccountSummary: Decodable, Identifiable, Equatable {
    let id: String
    let npub: String
    let displayName: String
    let signerKind: String
    let status: String
    var isActive: Bool { status == "active" }
}

struct PublishQueueEntry: Decodable, Identifiable, Equatable {
    let eventId: String
    let kind: UInt32
    let targetRelays: Int
    let status: String
    var id: String { eventId }
}

struct RelayEditRow: Decodable, Identifiable, Equatable {
    let url: String
    let role: String
    var id: String { url }
}

/// NIP-47 wallet connection status, projected from the kernel snapshot.
struct WalletStatusData: Decodable, Equatable {
    /// `"connecting"` | `"ready"` | `"error"` | `"disconnected"`
    let status: String
    let relayUrl: String
    let walletNpub: String
    let balanceMsats: UInt64?

    var isReady: Bool { status == "ready" }
    var isConnected: Bool { status == "connecting" || status == "ready" }

    var balanceSats: UInt64? {
        balanceMsats.map { $0 / 1000 }
    }
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
    let metadataSource: String?
    let source: String
}

struct TimelineItem: Decodable, Identifiable, Equatable, Hashable {
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

/// Full kernel metrics (matches nmp_core snapshot output). Timing fields are
/// optional because they are only populated once the relevant milestone is
/// reached (e.g., `firstEventMs` is nil until the first event arrives).
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

struct RelayStatus: Decodable, Equatable, Identifiable {
    var id: String { relayUrl }
    let role: String
    let relayUrl: String
    let connection: String
    let auth: String
    let nip77Negentropy: String?
    let activeWireSubscriptions: Int
    let reconnectCount: UInt32
    let lastConnectedAtMs: UInt64?
    let lastEventAtMs: UInt64?
    let lastNotice: String?
    let lastError: String?
    let bytesRx: UInt64?
    let bytesTx: UInt64?
}

extension Duration {
    var microseconds: Int {
        let parts = components
        return Int(parts.seconds) * 1_000_000 + Int(parts.attoseconds / 1_000_000_000_000)
    }
}
