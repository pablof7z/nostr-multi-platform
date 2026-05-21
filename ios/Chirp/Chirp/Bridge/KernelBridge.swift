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
        Self.configureStoragePath(for: raw)
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

    private static func configureStoragePath(for raw: UnsafeMutableRawPointer) {
        guard let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return
        }
        let directory = base.appendingPathComponent("NMP", isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            directory.path.withCString { nmp_app_set_storage_path(raw, $0) }
        } catch {
            kbLog.error("failed to create NMP storage directory: \(error.localizedDescription, privacy: .public)")
        }
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

    func createAccount(profile: [String: String], relays: [(String, String)], mls: Bool = true) {
        let profileJson = try! JSONSerialization.data(withJSONObject: profile, options: [])
        let profileStr = String(data: profileJson, encoding: .utf8)!
        let relaysJson = try! JSONSerialization.data(withJSONObject: relays.map { [$0.0, $0.1] }, options: [])
        let relaysStr = String(data: relaysJson, encoding: .utf8)!
        profileStr.withCString { profilePtr in
            relaysStr.withCString { relaysPtr in
                nmp_app_create_new_account(raw, profilePtr, relaysPtr, mls)
            }
        }
    }

    /// Publish a kind:0 profile metadata event for the active account through
    /// the kernel's `ActionModule` family. Routes via the single
    /// namespace-keyed `nmp_app_dispatch_action` entry point (`"nmp.publish"`
    /// namespace, `PublishAction::PublishProfile` JSON) — the kind:0 event,
    /// its `created_at` stamp, and signing are all owned by Rust (thin-shell
    /// rule: zero protocol logic in Swift). Fire-and-forget: matches the
    /// `publishNote` / `react` / `follow` pattern.
    func publishProfile(profile: [String: String]) {
        dispatchAction(
            namespace: "nmp.publish",
            body: ["PublishProfile": ["fields": profile]])
    }

    func switchActive(identityID: String) {
        identityID.withCString { nmp_app_switch_active(raw, $0) }
    }

    func removeAccount(identityID: String) {
        identityID.withCString { nmp_app_remove_account(raw, $0) }
    }

    /// Publish a kind:1 note (optionally a reply) through the kernel's
    /// `ActionModule` family. Routes via the single namespace-keyed
    /// `nmp_app_dispatch_action` entry point (`"nmp.publish"` namespace,
    /// `PublishAction::PublishNote` JSON) — the per-verb `nmp_app_publish_note`
    /// C symbol has been deleted. Fire-and-forget: the returned correlation
    /// JSON is freed and ignored.
    func publishNote(content: String, replyToID: String?) {
        let inner: [String: Any] = [
            "content": content,
            "reply_to_id": replyToID ?? NSNull(),
            "target": "Auto"
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: ["PublishNote": inner]),
              let jsonStr = String(data: data, encoding: .utf8) else {
            return
        }
        jsonStr.withCString { jsonPtr in
            "nmp.publish".withCString { nsPtr in
                if let ptr = nmp_app_dispatch_action(raw, nsPtr, jsonPtr) {
                    nmp_app_free_string(ptr)
                }
            }
        }
    }

    func retryPublish(handle: String) {
        handle.withCString { nmp_app_retry_publish(raw, $0) }
    }

    func cancelPublish(handle: String) {
        handle.withCString { nmp_app_cancel_publish(raw, $0) }
    }

    /// Dispatch a Chirp social-verb action through the generic
    /// `nmp_app_dispatch_action` path. `namespace` is one of `chirp.react` /
    /// `chirp.follow` / `chirp.unfollow` — registered by `nmp-app-chirp` at
    /// `nmp_app_chirp_register` time. `body` is the action JSON object.
    /// Fire-and-forget: the returned correlation JSON is freed and ignored;
    /// the outcome surfaces through the next kernel snapshot (matches the
    /// `publishNote` pattern — the per-verb C symbols have been deleted).
    private func dispatchAction(namespace: String, body: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let jsonStr = String(data: data, encoding: .utf8) else {
            return
        }
        jsonStr.withCString { jsonPtr in
            namespace.withCString { nsPtr in
                if let ptr = nmp_app_dispatch_action(raw, nsPtr, jsonPtr) {
                    nmp_app_free_string(ptr)
                }
            }
        }
    }

    func react(targetEventID: String, reaction: String) {
        dispatchAction(
            namespace: "chirp.react",
            body: ["target_event_id": targetEventID, "reaction": reaction])
    }

    func follow(pubkey: String) {
        dispatchAction(namespace: "chirp.follow", body: ["pubkey": pubkey])
    }

    func unfollow(pubkey: String) {
        dispatchAction(namespace: "chirp.unfollow", body: ["pubkey": pubkey])
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
///
/// There is one C callback for every capability; `ChirpCapabilities.handleJSON`
/// routes the request to the capability owning its `namespace` (keyring vs
/// HTTP). Rust invokes this from the actor thread (never the main thread), so
/// a synchronous capability like `HttpCapability` may block here safely.
private let nmpCapabilityCallback: NmpCapabilityCallback = { context, requestJSON in
    guard let context, let requestJSON else { return nil }
    let capabilities = Unmanaged<ChirpCapabilities>.fromOpaque(context).takeUnretainedValue()
    let requestStr = String(cString: requestJSON)
    let resultStr = capabilities.handleJSON(requestStr)
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
    // D0: the views cluster (`profile`, the visible timeline, `author_view`,
    // `thread_view`, and the `inserted` / `updated` / `removed` deltas) is no
    // longer a typed `KernelSnapshot` field set — all seven are surfaced
    // through the host-extensible `projections` map under built-in keys. The
    // stored decode for these fields is removed (a stored property would throw
    // `keyNotFound` and drop the entire snapshot at `decode`); computed
    // accessors below keep call sites (`KernelModel`) reading `update.profile`
    // / `update.items` / `update.authorView` etc. unchanged.
    let metrics: KernelMetrics
    // Single-relay backwards compat field alongside the array.
    let relayStatus: RelayStatus?
    let relayStatuses: [RelayStatus]
    // Perf diagnostics — optional so old kernels still decode (D1).
    let logicalInterests: [LogicalInterestStatus]?
    let wireSubscriptions: [WireSubscriptionStatus]?
    let logs: [String]?
    // D0: identity output (`accounts`, `active_account`) is no longer a typed
    // `KernelSnapshot` field — both are surfaced through the host-extensible
    // `projections` map under the built-in keys `"accounts"` /
    // `"active_account"`. Computed accessors below keep call sites
    // (`KernelModel`) reading `update.accounts` / `update.activeAccount`
    // unchanged.
    let lastErrorToast: String?
    // D0: NIP-47 NWC and NIP-46 remote signing are app nouns — neither is a
    // typed `KernelSnapshot` field anymore. Both are surfaced through the
    // kernel's host-extensible `projections` map: a built-in `"wallet"`
    // projection and a built-in `"bunker_handshake"` projection. The publish
    // cluster (`publish_queue`, `publish_outbox`, `relay_edit_rows`) is
    // likewise app-shaped relay/publish state and lives in the same map under
    // built-in keys. Optional so an older kernel that elides the map still
    // decodes (D1).
    let projections: SnapshotProjections?

    /// NIP-47 wallet projection — `projections["wallet"]`. Computed so call
    /// sites (`KernelModel`) keep reading `update.walletStatus` unchanged.
    var walletStatus: WalletStatusData? { projections?.wallet }

    /// NIP-46 bunker handshake progress — `projections["bunker_handshake"]`.
    /// Computed so call sites keep reading `update.bunkerHandshake` unchanged.
    var bunkerHandshake: BunkerHandshake? { projections?.bunkerHandshake }

    /// Publish queue projection — `projections["publish_queue"]`. Computed so
    /// call sites (`KernelModel`) keep reading `update.publishQueue` unchanged.
    var publishQueue: [PublishQueueEntry]? { projections?.publishQueue }

    /// Publish outbox projection — `projections["publish_outbox"]`. Computed so
    /// call sites keep reading `update.publishOutbox` unchanged.
    var publishOutbox: [PublishOutboxItem]? { projections?.publishOutbox }

    /// Relay-edit rows projection — `projections["relay_edit_rows"]`. Computed
    /// so call sites keep reading `update.relayEditRows` unchanged.
    var relayEditRows: [RelayEditRow]? { projections?.relayEditRows }

    /// Account list projection — `projections["accounts"]`. D0: identity
    /// output is no longer a typed snapshot field. Computed so call sites
    /// (`KernelModel`) keep reading `update.accounts` unchanged.
    var accounts: [AccountSummary]? { projections?.accounts }

    /// Active-account handle projection — `projections["active_account"]`.
    /// D0: identity output is no longer a typed snapshot field. Computed so
    /// call sites keep reading `update.activeAccount` unchanged.
    var activeAccount: String? { projections?.activeAccount }

    /// Per-tick action terminals — `projections["action_results"]` (direction
    /// review #29). `nil` in steady state; an array of every action that settled
    /// this tick when any did. Prefer this over `lastActionResult` for spinner
    /// management — the scalar drops terminals when two actions settle in one tick.
    var actionResults: [LastActionResult]? { projections?.actionResults }

    /// Most recent terminal action result — `projections["last_action_result"]`
    /// (direction review #24). Prefer `actionResults` (array) — this scalar
    /// silently drops terminals when two actions settle in the same kernel tick.
    var lastActionResult: LastActionResult? { projections?.lastActionResult }

    // ── D0 views cluster — projections-backed accessors ───────────────────
    //
    // The kernel no longer emits typed `profile` / `items` / `author_view` /
    // `thread_view` / `inserted` / `updated` / `removed` fields; all seven
    // live in `projections`. These computed accessors keep every call site
    // (`KernelModel.apply`, the feature views) reading `update.profile`,
    // `update.items`, etc. exactly as before.

    /// Active-account profile card — `projections["profile"]`. Falls back to a
    /// neutral placeholder card if a (legacy) kernel elides the projection, so
    /// the non-optional `KernelModel.profile` consumer never breaks.
    var profile: ProfileCard? { projections?.profile }

    /// Visible timeline — `projections["timeline"]` (the kernel renamed the
    /// generic `items` key to `timeline`). Non-optional with an empty default
    /// so the existing `update.items != items` change-detection in
    /// `KernelModel.apply` — which the modular-timeline refresh depends on —
    /// keeps working without an `Optional` unwrap. This deliberately differs
    /// from the identity-cluster optional pattern to preserve that flow.
    var items: [TimelineItem] { projections?.timeline ?? [] }

    /// Open author-view payload — `projections["author_view"]`. `nil` when no
    /// author view is open (kernel emits JSON null).
    var authorView: AuthorProfileSnapshot? { projections?.authorView }

    /// Open thread-view payload — `projections["thread_view"]`. `nil` when no
    /// thread view is open (kernel emits JSON null).
    var threadView: ThreadView? { projections?.threadView }

    /// Per-tick timeline delta — newly inserted items (`projections["inserted"]`).
    var inserted: [TimelineItem]? { projections?.inserted }

    /// Per-tick timeline delta — updated items (`projections["updated"]`).
    var updated: [TimelineItem]? { projections?.updated }

    /// Per-tick timeline delta — removed item ids (`projections["removed"]`).
    var removed: [String]? { projections?.removed }

    /// NIP-29 group-chat read model — `projections["nip29.group_chat"]`.
    /// `nil` until `nmp_app_chirp_register_group_chat` has wired a group's
    /// projection; an empty `messages` array once registered but no chat
    /// events have arrived. Computed so the `GroupChatStore` consumer keeps
    /// reading `update.groupChat` unchanged.
    var groupChat: GroupChatSnapshot? { projections?.groupChat }
}

/// The kernel's host-extensible `projections` map. Each built-in app-noun
/// projection (NWC wallet, NIP-46 bunker handshake, the publish cluster)
/// appears here under its own namespaced key instead of a typed
/// `KernelSnapshot` field (D0 — the protocol-neutral kernel emits app nouns
/// only through this map). Every member is optional: a host-registered
/// projection contributes JSON `null` when its feature is idle, the kernel-owned
/// publish cluster is always present once a kernel populates it, and the whole
/// map is absent on an older kernel build.
struct SnapshotProjections: Decodable, Equatable {
    let wallet: WalletStatusData?
    let bunkerHandshake: BunkerHandshake?
    let publishQueue: [PublishQueueEntry]?
    let publishOutbox: [PublishOutboxItem]?
    let relayEditRows: [RelayEditRow]?
    // D0: identity output. `accounts` decodes from `projections["accounts"]`;
    // `activeAccount` decodes from `projections["active_account"]` (the kernel
    // emits snake_case and the decoder uses `.convertFromSnakeCase`).
    let accounts: [AccountSummary]?
    let activeAccount: String?
    // Direction review #29: per-tick draining array of action terminals. Absent
    // in steady state; `[{correlationId, status, error?}]` whenever any action
    // settled this tick. Prefer this over the sticky scalar below.
    let actionResults: [LastActionResult]?
    // Direction review #24: sticky scalar — only the most recent terminal.
    // Kept for backward compat; prefer `actionResults` for multi-action ticks.
    let lastActionResult: LastActionResult?
    // D0: views cluster. The active-account `profile` card, the visible
    // `timeline` (the kernel renamed the generic `items` key to the more
    // descriptive `timeline`), the open-view `authorView` / `threadView`
    // payloads, and the per-tick `inserted` / `updated` / `removed` timeline
    // deltas are no longer typed `KernelSnapshot` fields — all seven are
    // built-in entries in this map. Every member is optional so an older
    // kernel that predates the migration still decodes (D1).
    let profile: ProfileCard?
    let timeline: [TimelineItem]?
    let authorView: AuthorProfileSnapshot?
    let threadView: ThreadView?
    let inserted: [TimelineItem]?
    let updated: [TimelineItem]?
    let removed: [String]?
    // NIP-29: the group-chat read projection registered by
    // `nmp_app_chirp_register_group_chat`. Its snapshot key is the dotted
    // string `"nip29.group_chat"`, which `.convertFromSnakeCase` cannot
    // derive from a Swift property name — hence the explicit `CodingKeys`
    // below (an explicit enum is all-or-nothing, so every other member is
    // re-listed there with its snake_case raw value).
    let groupChat: GroupChatSnapshot?

    /// Explicit coding keys.
    ///
    /// The decoder runs with `.convertFromSnakeCase`, which transforms each
    /// JSON key BEFORE it is matched against a `CodingKey.stringValue`. So
    /// every case here must carry the *post-transform* (camelCase) name —
    /// which is exactly the synthesized default — EXCEPT `groupChat`.
    ///
    /// The kernel's key is the dotted string `"nip29.group_chat"`.
    /// `.convertFromSnakeCase` splits on `_` only (`.` is opaque), so it
    /// maps `"nip29.group_chat"` → `"nip29.groupChat"`. That post-transform
    /// string is the raw value `groupChat` must declare; the synthesized
    /// default (`"groupChat"`) would never match.
    ///
    /// Declaring a `CodingKeys` enum overrides synthesis entirely, so every
    /// member is re-listed; all but `groupChat` simply restate the default.
    enum CodingKeys: String, CodingKey {
        case wallet
        case bunkerHandshake
        case publishQueue
        case publishOutbox
        case relayEditRows
        case accounts
        case activeAccount
        case actionResults
        case lastActionResult
        case profile
        case timeline
        case authorView
        case threadView
        case inserted
        case updated
        case removed
        case groupChat = "nip29.groupChat"
    }
}

// ─── NIP-29 group-chat read model ─────────────────────────────────────────
//
// Mirror of `nmp-nip29`'s `GroupChatSnapshot` / `GroupChatMessage` — the
// shape the `GroupChatProjection` serialises under the snapshot key
// `"nip29.group_chat"`. Thin-shell rule: these are pure DTOs; no Swift
// owns the ordering (the projection emits newest-first) or the membership
// filter (the projection matches kind + `h`-tag).

/// One rendered NIP-29 group-chat message. `pubkey` carries the event
/// author (hex); `kind` is one of 9 (chat) / 11 (discussion) / 1111
/// (comment). `id` is the event id (hex) and the stable list identity.
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// (inherited by every nested type) maps the kernel's `"created_at"` to
/// `createdAt` automatically. An explicit enum would have to spell the
/// post-transform name and is pure surface area — omitted deliberately.
struct GroupChatMessage: Decodable, Identifiable, Equatable {
    let id: String
    let pubkey: String
    let content: String
    let createdAt: UInt64
    let kind: UInt32
}

/// The serialised read-model a group-chat screen consumes. `messages` is
/// ordered newest-first (`created_at` descending, ties broken by id) by the
/// Rust projection — Swift does not re-sort.
struct GroupChatSnapshot: Decodable, Equatable {
    let messages: [GroupChatMessage]

    static let empty = GroupChatSnapshot(messages: [])
}

/// NIP-46 (`bunker://`) handshake progress, projected from the kernel snapshot
/// under `projections["bunker_handshake"]`. Stage values: `"connecting"`,
/// `"awaiting_pubkey"`, `"ready"`, `"failed"`, `"idle"`. `message` is a
/// human-readable progress / error hint.
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

/// One action terminal result. Used both in the per-tick `actionResults` array
/// (preferred) and the sticky `lastActionResult` scalar (deprecated — drops
/// terminals when two actions settle in the same kernel tick).
///
/// `status` is one of `"published"`, `"failed"`, `"cancelled"`. `error` is
/// `nil` for `published` / `cancelled` and carries a human-readable reason for
/// `failed` (the publish engine joins per-relay reasons with `; `).
///
/// To clear spinners correctly: iterate `update.actionResults` each tick
/// (direction review #29) — it drains every terminal that settled, not just
/// the last one.
struct LastActionResult: Decodable, Equatable {
    let correlationId: String
    let status: String
    let error: String?
}

struct PublishOutboxItem: Decodable, Identifiable, Equatable {
    let handle: String
    let eventId: String
    let kind: UInt32
    let title: String
    let preview: String
    let createdAtDisplay: String
    let status: String
    let targetRelays: Int
    let relays: [PublishOutboxRelay]

    var id: String { handle }
}

struct PublishOutboxRelay: Decodable, Identifiable, Equatable {
    let relayUrl: String
    let status: String
    let attempt: UInt32
    let message: String

    var id: String { relayUrl }
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
    let source: String
}

struct ProfileAction: Decodable, Equatable {
    let kind: String
    let label: String
    let targetPubkey: String
}

struct AuthorProfileSnapshot: Decodable, Equatable {
    let pubkey: String
    let state: String
    let profile: ProfileCard
    let items: [TimelineItem]
    let noteCount: Int
    let primaryAction: ProfileAction?
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
