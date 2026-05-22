import Darwin
import Foundation
import os.log

let kbLog = Logger(subsystem: "io.f7z.chirp", category: "KernelBridge")

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
    /// Opaque handle returned by `nmp_marmot_register`. The
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

    /// Wire the Rust update callback. `handler` runs on every snapshot frame;
    /// `onPanic` runs exactly once if/when the actor thread dies and the Rust
    /// supervisor emits an `{"t":"panic",...}` envelope on the update channel
    /// (D7 actor-death contract — see `crates/nmp-core/src/update_envelope.rs`).
    /// After `onPanic` fires the kernel is terminally dead for this process:
    /// no further snapshots will arrive and every subsequent FFI command is a
    /// silent no-op. The host (`KernelModel`) flips its `kernelIsDead`
    /// `@Published` flag and shows the red banner from `RootShell`.
    func listen(
        _ handler: @escaping (KernelUpdateResult) -> Void,
        onPanic: @escaping () -> Void = {}
    ) {
        let sink = KernelUpdateSink(handler: handler, onPanic: onPanic)
        updateSink = sink
        nmp_app_set_update_callback(raw, Unmanaged.passUnretained(sink).toOpaque(), nmpUpdateCallback)
    }

    /// Actor-liveness probe (D7 pull-side, ADR-0028). Returns `true` when the
    /// Rust actor thread is still running, `false` when it has terminated
    /// (panic, clean Shutdown, or null app). Pairs with the panic envelope
    /// signal `listen(_:onPanic:)` subscribes to: the host calls this on
    /// scenePhase = .active to catch the case where the push-side panic
    /// frame was missed (the app was backgrounded long enough for the Swift
    /// listener thread to exit before the host had a chance to react).
    func isAlive() -> Bool {
        nmp_app_is_alive(raw) == 1
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
    ///
    /// `callbackScheme` is the deep-link URL the signer app should open after
    /// approval (e.g. `"chirp://nip46"`). Rust chooses the relay from the
    /// kernel relay projection, percent-encodes the callback, and appends the
    /// `&callback=` query parameter. Swift supplies only platform callback
    /// information.
    func nostrConnectURI(callbackScheme: String? = nil) -> String? {
        if let cb = callbackScheme {
            return cb.withCString { cbPtr in
                guard let ptr = nmp_app_nostrconnect_uri(raw, nil, cbPtr) else {
                    return nil
                }
                defer { nmp_broker_free_string(ptr) }
                return String(cString: ptr)
            }
        }
        guard let ptr = nmp_app_nostrconnect_uri(raw, nil, nil) else {
            return nil
        }
        defer { nmp_broker_free_string(ptr) }
        return String(cString: ptr)
    }

    /// Dispatch a `nmp_app_create_new_account` call.
    ///
    /// The profile + relays are encoded through the `CreateAccountFFIPayload`
    /// `Codable` struct so the exact wire shape (`{"name":"…"}` + `[[url,role],…]`)
    /// is preserved while the encode path stays typed and defensible.
    ///
    /// Returns `nil` on success. Returns a human-readable error string on
    /// JSON-encode failure (typed-but-impossible for the `[String:String]` /
    /// `[(String,String)]` shapes we accept here, but we defend the boundary
    /// rather than trap with `try!`). Callers (`KernelModel.createAccount`)
    /// surface the error through the dispatch-error toast slot and abort the
    /// dispatch instead of crashing.
    @discardableResult
    func createAccount(
        profile: [String: String],
        relays: [(String, String)],
        mls: Bool = true
    ) -> String? {
        let payload = CreateAccountFFIPayload(profile: profile, relays: relays)
        let encoder = JSONEncoder()
        let profileStr: String
        let relaysStr: String
        do {
            let profileData = try encoder.encode(payload.profile)
            guard let str = String(data: profileData, encoding: .utf8) else {
                return "createAccount: failed to encode profile JSON as UTF-8"
            }
            profileStr = str
        } catch {
            return "createAccount: failed to encode profile (\(error.localizedDescription))"
        }
        do {
            let relaysData = try encoder.encode(payload.relays)
            guard let str = String(data: relaysData, encoding: .utf8) else {
                return "createAccount: failed to encode relays JSON as UTF-8"
            }
            relaysStr = str
        } catch {
            return "createAccount: failed to encode relays (\(error.localizedDescription))"
        }
        profileStr.withCString { profilePtr in
            relaysStr.withCString { relaysPtr in
                nmp_app_create_new_account(raw, profilePtr, relaysPtr, mls)
            }
        }
        return nil
    }

    /// Publish a kind:0 profile metadata event for the active account through
    /// the kernel's `ActionModule` family. Routes via the single
    /// namespace-keyed `nmp_app_dispatch_action` entry point (`"nmp.publish"`
    /// namespace, `PublishAction::PublishProfile` JSON) — the kind:0 event,
    /// its `created_at` stamp, and signing are all owned by Rust (thin-shell
    /// rule: zero protocol logic in Swift). PR-A: returns the synchronous
    /// dispatch result so the caller can drive a spinner keyed on the
    /// correlation_id (or surface the error envelope to the user).
    @discardableResult
    func publishProfile(profile: [String: String]) -> DispatchResult {
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
    /// C symbol has been deleted. PR-A: returns the synchronous dispatch
    /// result so the caller can drive a spinner keyed on the correlation_id
    /// (or surface the error envelope to the user). The terminal verdict
    /// arrives through `projections["action_results"]` on a later snapshot
    /// tick — match by `correlation_id` to clear the spinner.
    @discardableResult
    func publishNote(content: String, replyToID: String?) -> DispatchResult {
        let inner: [String: Any] = [
            "content": content,
            "reply_to_id": replyToID ?? NSNull(),
            "target": "Auto",
        ]
        return dispatchAction(namespace: "nmp.publish", body: ["PublishNote": inner])
    }

    func retryPublish(handle: String) {
        handle.withCString { nmp_app_retry_publish(raw, $0) }
    }

    func cancelPublish(handle: String) {
        handle.withCString { nmp_app_cancel_publish(raw, $0) }
    }

    /// Dispatch a Chirp social-verb action through the generic
    /// `nmp_app_dispatch_action` path. `namespace` is one of `chirp.react` /
    /// `nmp.follow` / `nmp.unfollow` — registered by `nmp-app-chirp` at
    /// `nmp_app_chirp_register` time. `body` is the action JSON object.
    ///
    /// PR-A: returns a `DispatchResult` parsed from the Rust-supplied JSON
    /// envelope so a host can drive a spinner keyed on the synchronous
    /// `correlation_id` (or surface the dispatch-rejection error). The
    /// terminal verdict — `"published"` / `"failed"` / `"cancelled"` — arrives
    /// asynchronously through `projections["action_results"]` on a later
    /// snapshot tick (match the `correlation_id` to clear the spinner).
    /// Before this change the Rust pointer was freed unread, leaving the host
    /// to race the next snapshot tick to discover the dispatch outcome.
    @discardableResult
    private func dispatchAction(namespace: String, body: [String: Any]) -> DispatchResult {
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let jsonStr = String(data: data, encoding: .utf8) else {
            return .failure("failed to serialize action body")
        }
        let envelope: String? = jsonStr.withCString { jsonPtr in
            namespace.withCString { nsPtr in
                guard let ptr = nmp_app_dispatch_action(raw, nsPtr, jsonPtr) else {
                    return nil
                }
                defer { nmp_app_free_string(ptr) }
                return String(cString: ptr)
            }
        }
        guard let envelope else {
            // D6: a non-null `app` never yields NULL — but the bridge is
            // defensive (a null KernelHandle would surface as nil here).
            return .failure("dispatch returned a null envelope")
        }
        return DispatchResult.parse(envelope: envelope)
    }

    @discardableResult
    func react(targetEventID: String, reaction: String) -> DispatchResult {
        dispatchAction(
            namespace: "chirp.react",
            body: ["target_event_id": targetEventID, "reaction": reaction])
    }

    @discardableResult
    func follow(pubkey: String) -> DispatchResult {
        dispatchAction(namespace: "nmp.follow", body: ["pubkey": pubkey])
    }

    @discardableResult
    func unfollow(pubkey: String) -> DispatchResult {
        dispatchAction(namespace: "nmp.unfollow", body: ["pubkey": pubkey])
    }

    /// Dispatch a NIP-57 zap through the `nmp.nip57.zap` ActionModule.
    /// Rust signs the kind:9734 zap request, completes the two-leg LNURL-pay
    /// round-trip, and (when the `wallet` feature is active) auto-dispatches
    /// `ActorCommand::WalletPayInvoice` so the bolt11 → NWC pay loop closes
    /// without a second host round-trip. The shell never sees the bolt11
    /// or parses LNURL/kind:9734 — thin-shell rule (aim.md §6.9).
    ///
    /// `lnurl` is the pre-extracted `authorLnurl` from the timeline item;
    /// `relays` is the receiver's preferred-relay set (today: the active
    /// account's read relays, falling back to `relay.damus.io` + `nos.lol`
    /// when the snapshot's relay list is empty). PR-A: returns the
    /// synchronous dispatch envelope so the host can drive a spinner keyed
    /// on the minted correlation_id.
    @discardableResult
    func zap(
        targetEventID: String,
        authorPubkey: String,
        lnurl: String,
        amountMsats: UInt64,
        relays: [String],
        comment: String? = nil
    ) -> DispatchResult {
        var body: [String: Any] = [
            "recipient_pubkey": authorPubkey,
            "amount_msats": amountMsats,
            "lnurl": lnurl,
            "relays": relays,
            "target_event_id": targetEventID,
        ]
        if let comment, !comment.isEmpty {
            body["comment"] = comment
        }
        return dispatchAction(namespace: "nmp.nip57.zap", body: body)
    }

    /// Generic dispatch entry-point keyed on a kernel-supplied
    /// `ProfileDispatchSpec`. The shell does NOT pick the namespace or build
    /// the body — Rust authored both inside `profile_action_for` (aim.md
    /// §4.4: writes flow through registered ActionModules, the shell binds
    /// blindly). `bodyJson` is the verbatim string the executor validates,
    /// passed straight to `nmp_app_dispatch_action` without re-serialisation.
    @discardableResult
    func dispatchRawAction(namespace: String, bodyJson: String) -> DispatchResult {
        let envelope: String? = bodyJson.withCString { jsonPtr in
            namespace.withCString { nsPtr in
                guard let ptr = nmp_app_dispatch_action(raw, nsPtr, jsonPtr) else {
                    return nil
                }
                defer { nmp_app_free_string(ptr) }
                return String(cString: ptr)
            }
        }
        guard let envelope else {
            return .failure("dispatch returned a null envelope")
        }
        return DispatchResult.parse(envelope: envelope)
    }

    /// PR-G — acknowledge a `correlation_id` in the `action_stages` snapshot
    /// mirror so the kernel drops its stage history. The host calls this AFTER
    /// reacting to the terminal stage (`Accepted` / `Failed`) — until acked the
    /// entry persists on every snapshot, so a dropped tick cannot strand the
    /// progress indicator. Dispatch is non-blocking (D8). A null / unknown
    /// correlation_id is a silent no-op (D6).
    func ackActionStage(_ correlationId: String) {
        correlationId.withCString { nmp_app_ack_action_stage(raw, $0) }
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

    @discardableResult
    func publishDmRelayList(relays: [String]) -> DispatchResult {
        dispatchAction(namespace: "nmp.nip17.publish_relay_list", body: ["relays": relays])
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
            kbLog.error("outer JSON parse failed bytes=\(data.count)")
            return nil
        }
        let frameTag = outer["t"] as? String
        guard frameTag == "snapshot" else {
            // Panic frames (t=panic) are intercepted earlier in
            // `nmpUpdateCallback` and never reach this decoder. Anything else
            // is a wire-format regression — log loudly so it surfaces in CI.
            kbLog.error("unknown envelope tag=\(frameTag ?? "<nil>") bytes=\(data.count)")
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
            kbLog.error("decode error: \(error.localizedDescription) bytes=\(innerData.count)")
            return nil
        }
    }
}

private final class KernelUpdateSink {
    let handler: (KernelUpdateResult) -> Void
    /// D7 actor-death hook. Runs exactly once when the Rust supervisor closure
    /// emits the `{"t":"panic",...}` envelope on the update channel before
    /// the actor thread (and the channel itself) drops. The host uses this to
    /// flip a `@Published` flag and show a fatal-error banner; the closure is
    /// the only Swift-side path that learns about an actor-thread panic from
    /// the update callback (since `nmpUpdateCallback` is a C `let` and cannot
    /// capture `self`).
    let onPanic: () -> Void

    init(
        handler: @escaping (KernelUpdateResult) -> Void,
        onPanic: @escaping () -> Void
    ) {
        self.handler = handler
        self.onPanic = onPanic
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
    let sink = Unmanaged<KernelUpdateSink>.fromOpaque(context).takeUnretainedValue()
    // D7 actor-death contract: the Rust supervisor emits exactly one
    // `{"t":"panic","v":{"msg":...}}` envelope before the channel closes.
    // The substring scan matches the wire shape pinned by the kernel test
    // `panic_frame_contains_panic_tag_substring` — that test is the source
    // of truth and is the contract this branch consumes.
    if payload.contains("\"t\":\"panic\"") {
        kbLog.fault("NMP_ACTOR_PANIC detected bytes=\(payload.utf8.count)")
        sink.onPanic()
        return
    }
    guard let result = KernelHandle.decode(pointer: pointer) else { return }
    sink.handler(result)
}

// ─── Swift-side timing wrapper ────────────────────────────────────────────

struct KernelUpdateResult {
    let update: KernelUpdate
    let payloadBytes: Int
    let callbackReceivedAt: ContinuousClock.Instant
    let decodeMicros: Int
}

// ─── dispatch_action return envelope (PR-A) ───────────────────────────────

/// Synchronous outcome of `nmp_app_dispatch_action`. The Rust kernel returns
/// `{"correlation_id":"<id>"}` on accept (the action was validated, minted a
/// correlation id, and routed to its executor), or `{"error":"<message>"}` on
/// reject (null app, unknown namespace, malformed JSON, module validator
/// rejection). PR-A: the Swift bridge parses this envelope so a caller can
/// drive a spinner keyed on the correlation_id and surface the error message
/// as a toast on the reject path.
///
/// The terminal verdict ("published" / "failed" / "cancelled") is a SEPARATE
/// async signal — match the `correlation_id` against
/// `projections["action_results"]` on subsequent snapshot ticks.
enum DispatchResult: Equatable {
    /// The action was accepted and enqueued. Carries the `correlation_id`
    /// minted by `ActionRegistry::start` — the host should add this to its
    /// `pendingActions` set and clear it when `action_results` reports the
    /// terminal verdict.
    case accepted(correlationId: String)
    /// The action was rejected synchronously. Carries the human-readable
    /// error from the Rust kernel — show it as a toast.
    case failure(_ message: String)

    var correlationId: String? {
        if case let .accepted(id) = self { return id }
        return nil
    }

    var errorMessage: String? {
        if case let .failure(msg) = self { return msg }
        return nil
    }

    /// Parse the JSON envelope returned by `nmp_app_dispatch_action`.
    ///
    /// The kernel's contract (`ffi/action.rs`): every non-null app returns
    /// either `{"correlation_id":"<32-hex or event-id>"}` or
    /// `{"error":"<reason>"}`. Anything else (malformed JSON, missing fields)
    /// degrades to `.failure` so the caller never silently loses an action.
    static func parse(envelope: String) -> DispatchResult {
        guard let data = envelope.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return .failure("dispatch envelope was not a JSON object (bytes=\(envelope.utf8.count))")
        }
        if let correlationId = object["correlation_id"] as? String, !correlationId.isEmpty {
            return .accepted(correlationId: correlationId)
        }
        if let message = object["error"] as? String {
            return .failure(message)
        }
        return .failure("dispatch envelope missing both correlation_id and error (bytes=\(envelope.utf8.count))")
    }
}

// ─── createAccount FFI payload (Codable, PR-L) ────────────────────────────

/// JSON payload for `nmp_app_create_new_account` — typed wrapper for the
/// profile metadata + onboarding relay seed list. The wire shape mirrors
/// what the Rust FFI expects exactly: a flat profile object
/// (`{"name":"…","about":"…"}`) and an array of two-element relay tuples
/// (`[["wss://…", "both"], …]`).
///
/// PR-L: replaces the `JSONSerialization.data(withJSONObject:)` + `try!`
/// path in `KernelBridge.createAccount` so a typed-but-impossible encode
/// failure surfaces as a toast instead of trapping the process.
struct CreateAccountFFIPayload: Encodable {
    let profile: [String: String]
    let relays: [[String]]

    init(profile: [String: String], relays: [(String, String)]) {
        self.profile = profile
        self.relays = relays.map { [$0.0, $0.1] }
    }
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
    // projection and a built-in `"bunker_handshake"` projection. The publish /
    // relay-settings cluster (`publish_queue`, `publish_outbox`,
    // `relay_edit_rows`, `relay_role_options`) is likewise app-shaped
    // relay/publish state and lives in the same map under built-in keys.
    // Optional so an older kernel that elides the map still decodes (D1).
    let projections: SnapshotProjections?

    /// NIP-47 wallet projection — `projections["wallet"]`. Computed so call
    /// sites (`KernelModel`) keep reading `update.walletStatus` unchanged.
    var walletStatus: WalletStatusData? { projections?.wallet }

    /// NIP-46 bunker handshake progress — `projections["bunker_handshake"]`.
    /// Computed so call sites keep reading `update.bunkerHandshake` unchanged.
    var bunkerHandshake: BunkerHandshake? { projections?.bunkerHandshake }

    /// NIP-46 onboarding read model — `projections["nip46_onboarding"]`. Carries
    /// the typed `stageKind` + pre-computed flags + the signer-app probe table
    /// the onboarding screen reads. Always present once the kernel has emitted
    /// a snapshot (the projection contributes a non-null payload on every tick).
    var nip46Onboarding: Nip46Onboarding? { projections?.nip46Onboarding }

    /// Publish queue projection — `projections["publish_queue"]`. Computed so
    /// call sites (`KernelModel`) keep reading `update.publishQueue` unchanged.
    var publishQueue: [PublishQueueEntry]? { projections?.publishQueue }

    /// Publish outbox projection — `projections["publish_outbox"]`. Computed so
    /// call sites keep reading `update.publishOutbox` unchanged.
    var publishOutbox: [PublishOutboxItem]? { projections?.publishOutbox }

    /// Outbox header summary — `projections["outbox_summary"]`. Pre-formatted
    /// title + subtitle + per-status counters (§6 anti-pattern #1). Computed
    /// so `NotificationsView` reads `update.outboxSummary` directly.
    var outboxSummary: OutboxSummary? { projections?.outboxSummary }

    /// Relay-edit rows projection — `projections["relay_edit_rows"]`. Computed
    /// so call sites keep reading `update.relayEditRows` unchanged.
    var relayEditRows: [RelayEditRow]? { projections?.relayEditRows }

    /// Relay-role picker options — `projections["relay_role_options"]`. Rust owns
    /// the canonical value list plus display labels/tint tokens.
    var relayRoleOptions: [RelayRoleOption]? { projections?.relayRoleOptions }

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

    /// PR-G: per-correlation_id stage history — `projections["action_stages"]`.
    /// `nil` in steady state; a `{correlation_id → [ActionStage...]}` map
    /// whenever any action's stages are tracked. Unlike `actionResults` (drained
    /// on emit) the same correlation_id reappears on every tick until the host
    /// calls `kernel.ackActionStage(_:)` — the race-protection guarantee that
    /// a dropped tick cannot strand a progress indicator.
    var actionStages: [String: [ActionStageEntry]]? { projections?.actionStages }

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

    /// Per-author mention payloads scoped to the open author-view items
    /// (`projections["mention_profiles"]`). Replaces the Swift
    /// `Dictionary(items.map { ... MentionProfile(...) })` derivation
    /// ProfileView used to build at body-time. Empty `[:]` when no author
    /// view is open; never nil for a current-schema kernel. Computed so
    /// the consumer keeps reading `update.mentionProfiles` unchanged.
    var mentionProfiles: [String: MentionProfileWire]? { projections?.mentionProfiles }

    /// NIP-29 group-chat read model — `projections["nmp.nip29.group_chat"]`.
    /// `nil` until `nmp_app_chirp_register_group_chat` has wired a group's
    /// projection; an empty `messages` array once registered but no chat
    /// events have arrived. Computed so the `GroupChatStore` consumer keeps
    /// reading `update.groupChat` unchanged.
    var groupChat: GroupChatSnapshot? { projections?.groupChat }

    /// NIP-17 DM inbox read model — `projections["nmp.nip17.dm_inbox"]`.
    /// `nil` until `nmp_app_chirp_register_dm_inbox` has wired the inbox
    /// projection; an empty `conversations` array once registered but no
    /// gift-wrap envelopes have arrived. Computed so the `DmInboxStore`
    /// consumer keeps reading `update.dmInbox` unchanged.
    var dmInbox: DmInboxSnapshot? { projections?.dmInbox }
    /// NIP-02 follow list — `projections["chirp.follow_list"]`.
    var followList: FollowListSnapshot? { projections?.followList }

    /// NIP-29 group-discovery read model —
    /// `projections["nmp.nip29.discovered_groups"]`. `nil` until
    /// `nmp_app_chirp_register_group_discovery` has wired a relay's
    /// projection; an empty `groups` array once registered but no
    /// kind:39000/39001/39002 events have arrived. Computed so the
    /// `DiscoveredGroupsStore` consumer keeps reading
    /// `update.discoveredGroups` unchanged.
    var discoveredGroups: DiscoveredGroupsSnapshot? { projections?.discoveredGroups }

    /// NIP-57 zap aggregate read model — `projections["nmp.nip57.zaps"]`.
    /// Wired by `nmp_app_chirp_register` (PR #288), which constructs a
    /// `ZapsAggregateProjection` and binds it as both a `KernelEventObserver`
    /// (ingest of kind:9735 receipts) and the snapshot-projection closure for
    /// this key. `nil` on a kernel build that predates the registration; an
    /// empty `totals` map once registered but no receipts have arrived.
    /// Computed so a future zap-count view binds to `update.zaps?.totals` the
    /// same way the chat / DM consumers bind to their snapshots.
    var zaps: ZapsAggregateSnapshot? { projections?.zaps }

    /// Diagnostics-screen read model — `projections["relay_diagnostics"]`
    /// (aim.md §4.5 / §6 anti-pattern #1 / §"Where do views live?" cleanup).
    /// One pre-rolled row per known relay URL with every aggregate (active /
    /// EOSE'd / total sub counts, total events_rx, byte counters) and every
    /// display string (relative-time labels, role / connection / auth
    /// labels + semantic tones) computed by `Kernel::relay_diagnostics_snapshot`.
    /// The three diagnostics views render fields directly — no `.filter` /
    /// `.sorted` / `.reduce` / `Date(timeIntervalSince1970:)`.
    /// `nil` only on a legacy kernel that predates the projection (D1).
    var relayDiagnostics: RelayDiagnosticsSnapshot? { projections?.relayDiagnostics }

    /// Settings-hub view payload — `projections["settings_hub"]`. Carries
    /// pre-formatted subtitles (currently just the relays count) the iOS
    /// Settings screen renders verbatim. `nil` only on a kernel older than
    /// this projection.
    var settingsHub: SettingsHubSummary? { projections?.settingsHub }
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
    // Built-in NIP-46 typed onboarding read model. Always populated by the
    // kernel (the underlying projection produces a non-null payload on every
    // tick); optional only so an older kernel build that predates the
    // projection still decodes (D1).
    let nip46Onboarding: Nip46Onboarding?
    let publishQueue: [PublishQueueEntry]?
    let publishOutbox: [PublishOutboxItem]?
    /// §6 anti-pattern #1 fix — pre-formatted outbox header (`"N pending
    /// publishes"` + per-status subtitle) computed in Rust. Optional so an
    /// older kernel that predates the projection still decodes (D1).
    let outboxSummary: OutboxSummary?
    let relayEditRows: [RelayEditRow]?
    let relayRoleOptions: [RelayRoleOption]?
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
    // PR-G: per-correlation_id stage mirror. Snake_case JSON key
    // `action_stages` decodes via `.convertFromSnakeCase` to `actionStages`.
    // The map is `correlation_id (String) → [ActionStageEntry]` ordered by
    // recording time. Absent when no correlation_id is currently tracked.
    let actionStages: [String: [ActionStageEntry]]?
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
    // string `"nmp.nip29.group_chat"`, which `.convertFromSnakeCase` cannot
    // derive from a Swift property name — hence the explicit `CodingKeys`
    // below (an explicit enum is all-or-nothing, so every other member is
    // re-listed there with its snake_case raw value).
    let groupChat: GroupChatSnapshot?
    // NIP-17: the DM inbox read projection registered by
    // `nmp_app_chirp_register_dm_inbox`. Its snapshot key is the dotted
    // string `"nmp.nip17.dm_inbox"` — same `.convertFromSnakeCase` caveat as
    // `groupChat`, handled by the explicit `CodingKeys` case below.
    let dmInbox: DmInboxSnapshot?
    // Chirp follow list — `projections["chirp.follow_list"]`. Registered by
    // `nmp_app_chirp_register_follow_list`. The dotted key is opaque to
    // `.convertFromSnakeCase` (it only replaces `_`), so the post-transform
    // key is `"chirp.followList"` — handled in the explicit `CodingKeys` below.
    let followList: FollowListSnapshot?

    // NIP-29: the group-discovery read projection registered by
    // `nmp_app_chirp_register_group_discovery`. Its snapshot key is the
    // dotted string `"nmp.nip29.discovered_groups"` — same `.convertFromSnakeCase`
    // caveat as `groupChat` / `dmInbox`, handled by the explicit
    // `CodingKeys` case below.
    let discoveredGroups: DiscoveredGroupsSnapshot?
    // NIP-57: the zap-aggregate read projection registered by
    // `nmp_app_chirp_register` (PR #288). Its snapshot key is the dotted
    // string `"nmp.nip57.zaps"`. `.convertFromSnakeCase` only splits on `_`,
    // and this key has none — the post-transform string is identical
    // (`"nmp.nip57.zaps"`), but the synthesized default for a Swift property
    // named `zaps` would be the bare string `"zaps"`. The explicit
    // `CodingKeys` case below is therefore mandatory.
    let zaps: ZapsAggregateSnapshot?
    // NIP-17: the DM relay-list projection registered by `register_dm_runtime`.
    // Its snapshot key is `"nmp.nip17.dm_relay_list"` — `.convertFromSnakeCase`
    // maps this to `"nmp.nip17.dmRelayList"`, handled by the explicit
    // `CodingKeys` case below.
    let dmRelayList: DmRelayListSnapshot?
    // Diagnostics roll-up — `projections["relay_diagnostics"]`. Built-in
    // kernel-owned projection (§4.5 / §6 anti-pattern #1 cleanup): replaces
    // the §"Where do views live?" violations the three diagnostics screens
    // committed (client-side filter / sorted / reduce / date math /
    // protocol-keyword switches). Always emitted by a current kernel build;
    // optional so a stale kernel still decodes.
    let relayDiagnostics: RelayDiagnosticsSnapshot?
    /// Per-author mention payload map — `projections["mention_profiles"]`.
    /// Replaces the Swift Dictionary derivation ProfileView used to build
    /// (`ProfileView.swift:28-40`); the Rust derivation lives in
    /// `Kernel::mention_profiles_from_items` (kernel/update.rs). Optional
    /// so an older kernel that pre-dates the projection still decodes (D1).
    let mentionProfiles: [String: MentionProfileWire]?
    // Settings-hub view payload — pre-formatted subtitles the iOS Settings
    // screen renders verbatim (aim.md §6/AP1: pluralization belongs in Rust).
    // Currently a single `relays_subtitle` field; further hub copy that
    // depends on substrate state will fold in here without adding a new
    // top-level projection key.
    let settingsHub: SettingsHubSummary?

    /// Explicit coding keys.
    ///
    /// The decoder runs with `.convertFromSnakeCase`, which transforms each
    /// JSON key BEFORE it is matched against a `CodingKey.stringValue`. So
    /// every case here must carry the *post-transform* (camelCase) name —
    /// which is exactly the synthesized default — EXCEPT `groupChat`.
    ///
    /// The kernel's keys are dotted strings — `"nmp.nip29.group_chat"` and
    /// `"nmp.nip17.dm_inbox"`. `.convertFromSnakeCase` splits on `_` only (`.`
    /// is opaque), so it maps `"nmp.nip29.group_chat"` → `"nmp.nip29.groupChat"`
    /// and `"nmp.nip17.dm_inbox"` → `"nmp.nip17.dmInbox"`. Those post-transform
    /// strings are the raw values `groupChat` / `dmInbox` must declare; the
    /// synthesized defaults would never match.
    ///
    /// Declaring a `CodingKeys` enum overrides synthesis entirely, so every
    /// member is re-listed; all but `groupChat` / `dmInbox` simply restate
    /// the default.
    enum CodingKeys: String, CodingKey {
        case wallet
        case bunkerHandshake
        case nip46Onboarding
        case publishQueue
        case publishOutbox
        case outboxSummary
        case relayEditRows
        case relayRoleOptions
        case accounts
        case activeAccount
        case actionResults
        case lastActionResult
        case actionStages
        case profile
        case timeline
        case authorView
        case threadView
        case inserted
        case updated
        case removed
        case groupChat = "nmp.nip29.groupChat"
        case dmInbox = "nmp.nip17.dmInbox"
        case followList = "chirp.followList"
        // `.convertFromSnakeCase` maps `"nmp.nip29.discovered_groups"` →
        // `"nmp.nip29.discoveredGroups"` (split on `_` only, `.` opaque) — that
        // is the post-transform string this case must declare.
        case discoveredGroups = "nmp.nip29.discoveredGroups"
        // `.convertFromSnakeCase` leaves `"nmp.nip57.zaps"` untouched (no `_`),
        // but declaring `CodingKeys` overrides synthesis entirely, so the raw
        // value must be the literal dotted kernel key — the synthesized default
        // would be the bare property name `"zaps"` and never match.
        case zaps = "nmp.nip57.zaps"
        // `.convertFromSnakeCase` maps `"nmp.nip17.dm_relay_list"` →
        // `"nmp.nip17.dmRelayList"` (split on `_` only, `.` opaque).
        case dmRelayList = "nmp.nip17.dmRelayList"
        case relayDiagnostics
        case mentionProfiles
        case settingsHub
    }
}

// ─── mention_profiles projection wire type ────────────────────────────────
//
// Per-author DTO bundled in `projections["mention_profiles"]`. Mirrors
// `nmp-core::kernel::types::MentionProfilePayload`. Thin-shell rule: a pure
// transport DTO — the projection's `MentionProfile` adapter below converts
// it to the existing rich struct used by `NoteRenderContext`. No Swift
// derives a `MentionProfile` from a `TimelineItem` anymore.

/// Wire shape for one entry in `projections["mention_profiles"]`.
/// `pictureUrl` is always non-empty (Rust falls back to the identicon URI),
/// so it surfaces as a plain `String` and the call site coerces to the
/// existing `MentionProfile.pictureUrl: String?` (empty → nil) at the
/// adapter boundary.
struct MentionProfileWire: Decodable, Equatable {
    let display: String
    let pictureUrl: String
    let avatarInitials: String
    let avatarColor: String
}

extension MentionProfile {
    /// Bridge from the kernel-supplied wire payload. An empty
    /// `picture_url` (which Rust never emits today — the placeholder URI is
    /// always populated) collapses to `nil` so the existing
    /// `MentionProfile.pictureUrl: String?` semantics stay unchanged.
    init(wire: MentionProfileWire) {
        self.init(
            display: wire.display,
            pictureUrl: wire.pictureUrl.isEmpty ? nil : wire.pictureUrl,
            initials: wire.avatarInitials,
            colorHex: wire.avatarColor
        )
    }
}

/// Settings-hub view projection — `projections["settings_hub"]`. The kernel
/// pre-formats every subtitle the Settings screen renders so the iOS shell
/// never owns the §6/AP1 pluralization / formatting copy. Decoded under
/// `.convertFromSnakeCase`, so the Rust `relays_subtitle` JSON key matches
/// the synthesized `relaysSubtitle` property name directly.
struct SettingsHubSummary: Decodable, Equatable {
    let relaysSubtitle: String

    static let empty = SettingsHubSummary(relaysSubtitle: "")
}

// ─── NIP-29 group-chat read model ─────────────────────────────────────────
//
// Mirror of `nmp-nip29`'s `GroupChatSnapshot` / `GroupChatMessage` — the
// shape the `GroupChatProjection` serialises under the snapshot key
// `"nmp.nip29.group_chat"`. Thin-shell rule: these are pure DTOs; no Swift
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

// ─── NIP-29 group-discovery read model ────────────────────────────────────
//
// Mirror of `nmp-nip29`'s `DiscoveredGroupsSnapshot` / `DiscoveredGroup` —
// the shape the `DiscoveredGroupsProjection` serialises under the snapshot
// key `"nmp.nip29.discovered_groups"`. Thin-shell rule: pure DTOs; no Swift
// owns the ordering (the projection emits alphabetical by `groupId`) or the
// member-count math (the projection counts `["p", _]` tags).

/// One discovered NIP-29 group, ready for `JoinGroupView` to render.
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// maps `"group_id"` / `"host_relay_url"` / `"member_count"` / `"admin_count"`
/// automatically.
struct DiscoveredGroup: Decodable, Identifiable, Equatable {
    /// The NIP-29 in-relay group id (the `["d", _]` tag value). Stable
    /// list identity inside `JoinGroupView`.
    let groupId: String
    /// The host relay this group lives on. NIP-29 identity is the pair
    /// `(host_relay_url, group_id)` — surfaced here so Swift can build a
    /// typed `GroupId` for the join action without re-supplying the URL.
    let hostRelayUrl: String
    let name: String?
    let picture: String?
    let about: String?
    let memberCount: UInt32
    let adminCount: UInt32
    let `public`: Bool
    let open: Bool

    var id: String { "\(hostRelayUrl)|\(groupId)" }
}

/// The serialised read-model `JoinGroupView` consumes. `groups` is ordered
/// alphabetically by `groupId` by the Rust projection — Swift does not
/// re-sort.
struct DiscoveredGroupsSnapshot: Decodable, Equatable {
    /// The host relay this snapshot describes — every row's `hostRelayUrl`
    /// equals this value (the projection is single-relay scoped).
    let hostRelayUrl: String
    let groups: [DiscoveredGroup]

    static let empty = DiscoveredGroupsSnapshot(hostRelayUrl: "", groups: [])
}

// ─── NIP-57 zap aggregate read model ──────────────────────────────────────
//
// Mirror of `nmp-nip57`'s `ZapsAggregateSnapshot` / `ZapCount` — the shape
// the `ZapsAggregateProjection` serialises under the snapshot key
// `"nmp.nip57.zaps"`. Thin-shell rule: these are pure DTOs. The Rust
// projection owns ALL protocol logic — kind:9735 receipt decoding, bolt11
// amount parsing, per-target grouping, and per-receipt dedupe. Swift never
// re-derives `count` or `totalMsats` from raw events.

/// Aggregate zap totals for a single target event. `totalMsats` sums the
/// authoritative bolt11 amount of every distinct receipt indexed under the
/// target; `count` is the number of distinct receipts. A receipt whose
/// amount could not be parsed contributes `0` msats but still increments
/// `count` — the zap *happened*, the amount is just unknown.
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// (inherited by every nested type) maps the kernel's `"total_msats"` to
/// `totalMsats` automatically.
struct ZapCount: Decodable, Equatable {
    let totalMsats: UInt64
    let count: UInt32
}

/// The serialised read-model a timeline-zap-count surface consumes.
/// `totals` maps a zapped event id (hex) to its running `ZapCount`. The
/// wrapper struct (rather than a bare map at the top level) mirrors the
/// Rust shape and leaves room for sibling fields without a breaking
/// re-shape.
struct ZapsAggregateSnapshot: Decodable, Equatable {
    /// `target_event_id (hex) → ZapCount`. Empty when the projection has
    /// been registered but no kind:9735 receipts have arrived yet.
    let totals: [String: ZapCount]

    static let empty = ZapsAggregateSnapshot(totals: [:])
}

// ─── NIP-17 DM relay-list read model ─────────────────────────────────────
//
// Mirror of the `DmRelayListSnapshot` the `DmRuntimeController` serialises
// under the snapshot key `"nmp.nip17.dm_relay_list"`. Thin-shell rule: pure
// DTO — the Rust side owns all kind:10050 reconciliation logic.

/// The active account's DM relay list state. `activePubkey` is the active
/// account's hex pubkey (nil when no account is loaded). `readRelayUrls`
/// is the subset of configured relay URLs eligible for DM reads.
///
/// No explicit `CodingKeys`: `.convertFromSnakeCase` maps `"active_pubkey"` →
/// `activePubkey` and `"read_relay_urls"` → `readRelayUrls` automatically.
struct DmRelayListSnapshot: Decodable, Equatable {
    let activePubkey: String?
    let readRelayUrls: [String]
}

// ─── NIP-17 DM inbox read model ───────────────────────────────────────────
//
// Mirror of `nmp-nip17`'s `DmInboxSnapshot` / `DmConversation` / `DmMessage`
// — the shape the `DmInboxProjection` serialises under the snapshot key
// `"nmp.nip17.dm_inbox"`. Thin-shell rule: these are pure DTOs. The Rust
// projection owns ALL protocol logic — NIP-44 decryption, kind:14 filtering,
// per-peer grouping, and newest-first ordering. Swift never re-sorts or
// re-groups.

/// One decrypted NIP-17 direct message. `senderPubkey` is taken from the
/// verified kind:13 seal (not a forgeable tag); `id` is the inner rumor
/// event id (hex) and the stable list identity. `isOutgoing` is pre-
/// classified by the Rust projection against the active local pubkey —
/// the shell never compares pubkeys to align a bubble (thin-shell rule).
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// maps `"sender_pubkey"` / `"created_at"` / `"reply_to"` / `"is_outgoing"` /
/// `"source_relays"` automatically.
struct DmMessage: Decodable, Identifiable, Equatable {
    let id: String
    let senderPubkey: String
    let content: String
    let createdAt: UInt64
    let replyTo: String?
    let isOutgoing: Bool
    let sourceRelays: [String]?
}

/// One DM thread — every message exchanged with a single peer. `messages`
/// is ordered chronologically by the Rust projection — oldest first,
/// newest last — so the host renders a chat log in that order and never
/// reverses (thin-shell rule). The thread's most-recent message is
/// `messages.last`.
///
/// Display fields (`peerNpub`, `peerShortNpub`, `peerAvatarInitials`,
/// `peerAvatarColor`) are computed in Rust at snapshot time — the shell
/// renders them directly (thin-shell rule, no bech32 encoding in Swift).
struct DmConversation: Decodable, Identifiable, Equatable {
    /// The OTHER party in the thread (hex pubkey). Also the list identity.
    let peerPubkey: String
    /// Full bech32 `npub1…` encoding of `peerPubkey`. For copy/paste.
    let peerNpub: String
    /// Abbreviated bech32: 10-head + "…" + 6-tail. Ready for display rows.
    let peerShortNpub: String
    /// Two-char uppercase initials for the avatar tile.
    let peerAvatarInitials: String
    /// Six-hex deterministic avatar background colour (no `#` prefix).
    let peerAvatarColor: String
    let messages: [DmMessage]

    var id: String { peerPubkey }
}

// ─── NIP-02 follow list read model ───────────────────────────────────────────
//
// Mirror of `nmp-app-chirp`'s `FollowListProjection` — the shape it serialises
// under the snapshot key `"chirp.follow_list"`. All display strings are
// computed in Rust; Swift renders what it receives (thin-shell rule).

/// One entry in the active account's follow list.
struct FollowEntry: Decodable, Identifiable, Equatable {
    let pubkey: String
    let npub: String
    let shortNpub: String
    let avatarInitials: String
    let avatarColor: String
    var id: String { pubkey }
}

/// The serialised follow-list snapshot. `follows` is the active account's
/// NIP-02 kind:3 contact list, each entry pre-formatted for display.
struct FollowListSnapshot: Decodable, Equatable {
    let follows: [FollowEntry]
    static let empty = FollowListSnapshot(follows: [])
}

/// The serialised read-model the DM screens consume. `conversations` is
/// ordered by most-recent message (newest thread first) by the Rust
/// projection — Swift does not re-sort.
struct DmInboxSnapshot: Decodable, Equatable {
    let conversations: [DmConversation]

    static let empty = DmInboxSnapshot(conversations: [])
}

// ─── Diagnostics read model (relay_diagnostics projection) ────────────────
//
// Mirror of `nmp-core::kernel::relay_diagnostics::RelayDiagnosticsSnapshot` —
// the shape the `relay_diagnostics` built-in projection emits under the
// snapshot key `"relay_diagnostics"`. The Rust projection pre-rolls every
// aggregate (active / EOSE'd / total sub counts, total events_rx) and pre-
// formats every display string (relative-time labels, role / connection /
// auth labels + semantic tones).
//
// Thin-shell rule: these are pure DTOs. The shell renders fields directly —
// it does NOT filter / sort / reduce wireSubscriptions, does NOT compute
// `Date(timeIntervalSince1970:)` from `lastEventAtMs`, does NOT switch on
// `state == "open"` to pick a color. All of that is in the Rust projection
// (aim.md §4.5 / §6 anti-pattern #1 / §"Where do views live?" — line 241).

/// Per-wire-subscription enriched row.
struct RelayDiagnosticsWireSub: Decodable, Identifiable, Equatable {
    let wireId: String
    let shortWireId: String
    let relayUrl: String
    let filterSummary: String
    let stateLabel: String
    let stateTone: String
    let consumerCountLabel: String
    let eventsRxDisplay: String?
    let eoseObserved: Bool
    let openedDisplay: String
    let lastEventDisplay: String?
    let eoseDisplay: String?
    let closeReason: String?
    var id: String { wireId }
}

/// One rolled-up relay row.
struct RelayDiagnosticsRow: Decodable, Identifiable, Equatable {
    let relayUrl: String
    let shortUrl: String
    let roleLabel: String
    let roleTone: String
    let connectionLabel: String
    let connectionTone: String
    let authLabel: String
    let authTone: String
    let totalSubCount: UInt32
    let activeSubCount: UInt32
    let eosedSubCount: UInt32
    let totalEventsRx: UInt64
    let totalEventsDisplay: String
    let reconnectCount: UInt32
    let bytesRxDisplay: String?
    let bytesTxDisplay: String?
    let lastConnectedDisplay: String?
    let lastEventDisplay: String?
    let lastNotice: String?
    let lastError: String?
    let wireSubs: [RelayDiagnosticsWireSub]
    var id: String { relayUrl }
}

/// Logical interest with semantic tone pre-classified.
struct RelayDiagnosticsInterest: Decodable, Identifiable, Equatable {
    let key: String
    let state: String
    let stateTone: String
    let refcount: UInt32
    let cacheCoverage: String
    let relayUrls: [String]
    var id: String { key }
}

/// Top-level diagnostics snapshot.
struct RelayDiagnosticsSnapshot: Decodable, Equatable {
    let relays: [RelayDiagnosticsRow]
    let interests: [RelayDiagnosticsInterest]

    static let empty = RelayDiagnosticsSnapshot(relays: [], interests: [])
}

/// NIP-46 (`bunker://`) handshake progress, projected from the kernel snapshot
/// under `projections["bunker_handshake"]`. Stage values: `"connecting"`,
/// `"awaiting_pubkey"`, `"ready"`, `"failed"`, `"idle"`. `message` is a
/// human-readable progress / error hint.
///
/// **Prefer `Nip46Onboarding` for the onboarding UI**: that projection carries
/// the typed `stageKind` enum + pre-computed `isInFlight` / `isFailed` /
/// `isTerminalSuccess` / `canCancel` flags. For the `AccountsView` "Add
/// account" sheet (and any other site that already reads
/// `model.bunkerHandshake`), the same flags are now mirrored on this struct
/// too: doctrine §6 anti-pattern #1 + RMP bible commandment #4 — shells
/// render fields directly instead of switching on the raw `stage` string.
///
/// The flag / label fields are optional so an older kernel build that
/// predates the doctrine fix still decodes (D1); call sites that fall back
/// to `stage` are correct (but should migrate once the kernel rebuild lands).
struct BunkerHandshake: Decodable, Equatable {
    let stage: String
    let message: String?
    /// `stage == "idle"` (computed Rust-side; absent on legacy kernels).
    let isIdle: Bool?
    /// `stage` is one of `"connecting"` / `"awaiting_pubkey"`. Drives the
    /// spinner vs. terminal-icon swap and input-disabled gates.
    let isInFlight: Bool?
    /// `stage == "failed"`. Drives the red triangle + "Retry" button label.
    let isFailed: Bool?
    /// `stage == "ready"`. Drives the green check on the progress row.
    let isTerminalSuccess: Bool?
    /// True when the handshake can be cancelled (i.e. mid-flight). Drives
    /// the visibility of the "Cancel handshake" button.
    let canCancel: Bool?
    /// Pre-formatted English label (e.g. `"Connecting to bunker relays…"`).
    /// Always non-empty when emitted by a current kernel; legacy kernels
    /// (pre-projection) leave it `nil` — call sites fall back on `stage`.
    let stageLabel: String?
}

/// NIP-46 onboarding read model — `projections["nip46_onboarding"]`.
///
/// Rust owns the entire onboarding state machine and pre-computes every value
/// a host UI reads: the static signer-app probe table, the typed `stageKind`,
/// and the boolean flags shells use to render spinners / icons / buttons.
/// Views never string-compare stage values; they read `stageKind` directly.
///
/// Always present (the projection contributes a non-null payload on every
/// tick) so `signerApps` is reachable even when no handshake is in flight.
struct Nip46Onboarding: Decodable, Equatable {
    /// One row of the signer-app table. Rust owns the URL schemes that
    /// qualify as NIP-46 compatible; Swift only iterates and calls
    /// `UIApplication.canOpenURL` (a platform capability per aim.md §4.6).
    struct SignerApp: Decodable, Equatable, Identifiable {
        let scheme: String
        let displayLabel: String
        let signerKind: String
        var id: String { scheme }
    }

    /// Typed stage token. `nil` when no handshake is in flight (mirrors the
    /// `bunker_handshake` slot's empty state). `unknown` is the forward-compat
    /// fall-through for any wire stage the host hasn't been re-typed against.
    enum StageKind: String, Decodable, Equatable {
        case idle
        case connecting
        case awaitingPubkey = "awaiting_pubkey"
        case ready
        case failed
        case unknown
    }

    let signerApps: [SignerApp]
    let stageKind: StageKind?
    let progressMessage: String?
    let isInFlight: Bool
    let isFailed: Bool
    let isTerminalSuccess: Bool
    let canCancel: Bool
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
    /// Pre-formatted "Show N earlier note(s)" string. Empty when `previousCount == 0`.
    /// Rust owns pluralization — host renders verbatim (aim.md §6 anti-pattern #1).
    /// Optional for forward-compatibility with older kernel builds that predate
    /// the field; the host treats `nil` as the empty string (D1 — never branch on
    /// missing protocol output, render placeholder instead).
    let previousCountLabel: String?
    /// Pre-formatted "N more repl{y,ies}" string. Empty when `nextCount == 0`.
    /// Same rationale as `previousCountLabel`.
    let nextCountLabel: String?
}

struct AccountSummary: Decodable, Identifiable, Equatable {
    let id: String
    let npub: String
    let displayName: String
    /// Stable wire token (`"local"` | `"nip46"` | …). Kept for the diagnostics
    /// surface that still renders the raw string; new view code MUST bind
    /// `signerLabel` / `signerIsRemote` instead (aim.md §4.4 / §4.5).
    let signerKind: String
    /// Stable status token (`"active"` | `"idle"`). Kept for backward compat;
    /// new view code MUST bind `isActive` instead.
    let status: String
    /// Pre-classified, human-readable label rendered verbatim by the UI.
    /// Replaces the old `switch kind.lowercased() { … }` in AccountsView.
    let signerLabel: String
    /// `true` when the signer's key material lives outside the kernel
    /// (NIP-46 bunker today, NIP-07 / hardware later). Replaces
    /// `.filter { $0.signerKind.lowercased() == "nip46" }` in AccountsView.
    let signerIsRemote: Bool
    /// Pre-derived `status == "active"`. Native binds this directly.
    let isActive: Bool
    /// Profile picture URL from kind:0 metadata; nil when not yet loaded.
    let pictureUrl: String?
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

// ─── PR-G: action_stages projection wire type ────────────────────────────
//
// One entry in a correlation_id's stage history. The Rust side uses serde
// `#[serde(tag = "stage", rename_all = "snake_case")]` so the `stage`
// discriminant ships as a flat snake_case string ("requested",
// "publishing", "accepted", "failed"). `Failed` carries a sibling
// `reason` field; other variants do not. `at_ms` is the Unix epoch
// millisecond stamp at recording time (kernel clock, deterministic under
// replay). `detail` is opaque per-stage JSON the host renders verbatim
// — `nil` when the kernel emitted no detail.
//
// To preserve the JSON-decoded `detail` as opaque data, we use
// `AnyCodableValue` (an existing helper in this file) or a `JSONValue`
// wrapper. Since the host largely doesn't introspect `detail` today, a
// `Data?`-style passthrough is sufficient: decode as `String?` of the
// JSON serialization. For PR-G the renderer needs only `stage` and
// `reason`; carrying `detail` as `[String: AnyDecodable]` is future
// work.

/// One stage in an async action's lifecycle, decoded from one entry of
/// `projections["action_stages"][<correlation_id>][i]`.
///
/// Construction-time decoding is forgiving: any unrecognized `stage`
/// discriminant collapses to `.unknown(raw:)` so a future kernel stage
/// added without a Swift counterpart does not crash the bridge (D1 —
/// snapshot decoders must degrade gracefully on schema growth).
enum ActionStage: Equatable {
    case requested
    case awaitingCapability
    case publishing
    case accepted
    /// `reason` is the human-readable failure message the host renders
    /// verbatim. Mirrors the `error` field on `LastActionResult`.
    case failed(reason: String)
    /// Catchall for future kernel stages — preserves the raw tag so a
    /// diagnostic view can still display something meaningful.
    case unknown(raw: String)

    var isTerminal: Bool {
        switch self {
        case .accepted, .failed: return true
        default: return false
        }
    }
}

/// One row in a correlation_id's stage history. The PR-G snapshot mirror
/// projection emits a `[String: [ActionStageEntry]]` map; this struct
/// decodes one element of the inner array.
struct ActionStageEntry: Decodable, Equatable {
    let stage: ActionStage
    /// Unix epoch milliseconds — when the kernel reducer recorded the
    /// transition. Stable under `FixedClock` for deterministic replay.
    let atMs: UInt64

    enum CodingKeys: String, CodingKey {
        case stage
        case atMs
        case reason
        // `detail` is intentionally not decoded — the bridge passes the
        // stage forward verbatim without introspection. Future work can
        // add a typed `detail` field per-stage.
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let raw = try container.decode(String.self, forKey: .stage)
        atMs = try container.decode(UInt64.self, forKey: .atMs)
        switch raw {
        case "requested": stage = .requested
        case "awaiting_capability", "awaitingCapability": stage = .awaitingCapability
        case "publishing": stage = .publishing
        case "accepted": stage = .accepted
        case "failed":
            let reason = try container.decodeIfPresent(String.self, forKey: .reason) ?? ""
            stage = .failed(reason: reason)
        default:
            stage = .unknown(raw: raw)
        }
    }
}

struct PublishOutboxItem: Decodable, Identifiable, Equatable {
    let handle: String
    let eventId: String
    let kind: UInt32
    let title: String
    let preview: String
    let createdAtDisplay: String
    let status: String
    /// Pre-formatted English status label (e.g. `"Sending"`, `"Retrying"`).
    /// Doctrine §6 anti-pattern #1: the shell renders this verbatim — it
    /// never `switch`es on `status` to choose a label string. Always non-empty.
    let statusLabel: String
    /// SF Symbol name pre-classified from the Nostr `kind` in Rust. The view
    /// passes this directly to `Image(systemName:)` — it never branches on
    /// `kind` to pick an icon (aim.md §4.4 / §6 anti-pattern: kind-number
    /// switches in Swift). Always non-empty (default `"doc.text"`).
    let systemImage: String
    /// Pre-decided "is the Retry button enabled" flag. The kernel owns the
    /// retry-policy rule ("a row already sending cannot be retried"); the
    /// shell binds this directly to `.disabled(!canRetry)` (RMP bible
    /// commandment #4 — no native `if` deciding what the app should do).
    let canRetry: Bool
    let targetRelays: Int
    /// Pre-formatted "N relays · <created_at>" header line. Server-side
    /// pluralization keeps the shell free of the `count == 1 ? "" : "s"`
    /// ternary (§6 anti-pattern #1).
    let targetSummary: String
    let relays: [PublishOutboxRelay]

    var id: String { handle }
}

struct PublishOutboxRelay: Decodable, Identifiable, Equatable {
    let relayUrl: String
    let status: String
    /// Pre-formatted English status label (e.g. `"Sending"`, `"Retrying"`).
    /// Always non-empty — the shell renders this verbatim, never
    /// `.capitalized`s the wire `status` key or switches on it.
    let statusLabel: String
    let attempt: UInt32
    /// Pre-formatted "try N" badge text — empty when `attempt == 0` so the
    /// shell renders unconditionally (D1: best-effort rendering, no
    /// `if attempt > 0` branch). When non-empty the shell renders it as-is.
    let attemptLabel: String
    let message: String

    var id: String { relayUrl }
}

/// Pre-formatted outbox-summary header (title + subtitle) plus per-status
/// counters. Doctrine §6 anti-pattern #1 ("Duplicated formatting logic
/// across platforms") + RMP bible commandment #4 ("no native business
/// logic"). The shell binds `title` / `subtitle` directly — it never
/// `.filter`-counts `publishOutbox` to derive them.
struct OutboxSummary: Decodable, Equatable {
    let title: String
    let subtitle: String
    let total: UInt32
    let sending: UInt32
    let retrying: UInt32
    let queued: UInt32
    let failed: UInt32

    /// Empty-state fallback used when the snapshot predates the projection
    /// (an older kernel build that ships no `outbox_summary` key). Mirrors
    /// the Rust `outbox_summary_snapshot` empty-outbox shape so the shell
    /// never has to reconstruct the strings.
    static let empty = OutboxSummary(
        title: "Nothing waiting",
        subtitle: "Your local outbox is clear.",
        total: 0,
        sending: 0,
        retrying: 0,
        queued: 0,
        failed: 0
    )
}

struct RelayEditRow: Decodable, Identifiable, Equatable {
    let url: String
    let role: String
    let roleLabel: String
    let roleTint: String
    var id: String { url }

    init(
        url: String,
        role: String,
        roleLabel: String = "",
        roleTint: String = "accent"
    ) {
        self.url = url
        self.role = role
        self.roleLabel = roleLabel
        self.roleTint = roleTint
    }
}

struct RelayRoleOption: Decodable, Identifiable, Equatable {
    let value: String
    let label: String
    let tint: String
    let isDefault: Bool

    var id: String { value }
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
    /// Pre-truncated display form Rust formats with the `<first10>…<last8>`
    /// policy. The shell binds this verbatim — no Swift-side truncation helper
    /// (aim.md §6.9, Chirp thin-shell: zero display formatting in Swift).
    let npubShort: String
    let display: String
    let pictureUrl: String?
    let nip05: String
    let about: String
    let avatarInitials: String
    let avatarColor: String
    let source: String
    let hasProfile: Bool
    /// NIP-57 lightning address (`lud16`) / LNURL (`lud06`) pre-extracted
    /// from kind:0. `nil` when the user has no lightning address or their
    /// kind:0 hasn't arrived. The zap button is shown only when this is
    /// non-nil — Rust decides zapability, Swift renders (thin-shell rule).
    let lnurl: String?
}

/// Dispatch spec for a `ProfileAction` that fires a write through
/// `nmp_app_dispatch_action`. Present for follow / unfollow, absent for the
/// local-UI `edit_profile` intent. The shell branches on
/// `profileAction.dispatch != nil`, never on `kind` — aim.md §4.4 forbids a
/// Swift `switch action.kind { … }` deciding which write to perform.
struct ProfileDispatchSpec: Decodable, Equatable {
    let namespace: String
    let bodyJson: String
}

struct ProfileAction: Decodable, Equatable {
    /// Stable discriminator preserved for diagnostics/tests. The shell must
    /// NOT switch on this — branch on `dispatch` instead.
    let kind: String
    let label: String
    let targetPubkey: String
    /// SF Symbol name the shell renders without further mapping.
    let iconName: String
    /// Present for write actions; absent for local intents (edit sheet).
    let dispatch: ProfileDispatchSpec?
}

struct AuthorProfileSnapshot: Decodable, Equatable {
    let pubkey: String
    let state: String
    let profile: ProfileCard
    let items: [TimelineItem]
    let noteCount: Int
    /// Pre-formatted post-count string the shell binds verbatim
    /// (e.g. `"5"`). Rust owns the format so the shell never derives display
    /// state from the items array (aim.md §6.9).
    let noteCountDisplay: String
    let primaryAction: ProfileAction?
}

struct TimelineItem: Decodable, Identifiable, Equatable, Hashable {
    let id: String
    let authorPubkey: String
    let authorDisplay: String
    let authorPictureUrl: String?
    let authorAvatarInitials: String
    let authorAvatarColor: String
    /// NIP-57 lightning address (`lud16`) / LNURL (`lud06`) pre-extracted
    /// from the author's kind:0 metadata. `nil` when the author has no
    /// lightning address or their kind:0 hasn't arrived yet. The shell
    /// zap button toggles its enabled/disabled state on this value;
    /// Swift never parses raw metadata (thin-shell rule, aim.md §6.9).
    let authorLnurl: String?
    /// Nostr event kind (1 = note, 6 = repost, 7 = reaction, …). The kernel
    /// supplies this so the shell can render kind-conditional UI (e.g. a
    /// "Repost" badge or alternate navigation target) without re-parsing the
    /// raw event JSON in `content`. Thin-shell rule: the kind is the
    /// authoritative protocol signal — never inferred from content shape.
    ///
    /// Prefer `isRepost` for branching the UI. The raw `kind` is retained for
    /// diagnostics and forward-compatible decoders that need the full integer
    /// (e.g. `NoteContentView`'s typed `MediaKind` switch is unrelated and
    /// stays put). The thin-shell rule is enforced by the `isRepost` bool —
    /// the view layer must NOT `switch` on this integer to derive display
    /// state. See `aim.md §6.9` (Chirp thin-shell).
    let kind: UInt32
    let content: String
    let contentPreview: String
    let createdAtDisplay: String
    let relayCount: UInt32
    /// `true` when this row represents a NIP-18 repost (kind:6). Rust
    /// pre-computes this so the view layer never re-derives protocol
    /// semantics from `kind`. Decoded with `#[serde(default)]` semantics on
    /// the kernel side — a pre-existing snapshot without the field decodes
    /// as `false`.
    let isRepost: Bool
    /// Event id to navigate to when the row is tapped. For a kind:1 note
    /// this is `id`; for a kind:6 repost it is the inner kind:1's id when
    /// the NIP-18 embedded JSON is well-formed, falling back to `id` when
    /// the inner event is missing/malformed. The shell binds this verbatim —
    /// it MUST NOT parse `content` to find the inner event id.
    let navTargetId: String
    /// Inner-note text rendered inside a kind:6 repost cell. Empty string
    /// for kind:1 rows (the cell uses `content` directly). For kind:6 it is
    /// the inner event's `content` string when the NIP-18 embedded JSON
    /// parses, or `""` when it is missing/malformed. The shell uses this
    /// verbatim — no JSON parsing in Swift.
    let repostInnerContent: String
}

extension TimelineItem {
    // Decoder is tolerant of forward/backward schema drift so an older
    // kernel snapshot (no `is_repost` etc.) still decodes — falls back to
    // `false` / `""` / `id`, mirroring the Rust fallbacks bit-for-bit. The
    // outer `KernelSnapshot` decoder runs with `.convertFromSnakeCase`, so
    // JSON `is_repost` → property `isRepost` (post-transform name) without
    // an explicit raw value.
    //
    // The decoder lives in an `extension` so the auto-synthesized memberwise
    // initializer is preserved for synthetic construction sites (e.g.
    // `ModularBlockView.syntheticItem`) — adding `init(from:)` to the body
    // would suppress it.
    private enum CodingKeys: String, CodingKey {
        case id, authorPubkey, authorDisplay, authorPictureUrl
        case authorAvatarInitials, authorAvatarColor, authorLnurl
        case kind, content, contentPreview, createdAtDisplay, relayCount
        case isRepost, navTargetId, repostInnerContent
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let id = try c.decode(String.self, forKey: .id)
        self.init(
            id: id,
            authorPubkey: try c.decode(String.self, forKey: .authorPubkey),
            authorDisplay: try c.decode(String.self, forKey: .authorDisplay),
            authorPictureUrl: try c.decodeIfPresent(String.self, forKey: .authorPictureUrl),
            authorAvatarInitials: try c.decode(String.self, forKey: .authorAvatarInitials),
            authorAvatarColor: try c.decode(String.self, forKey: .authorAvatarColor),
            // NIP-57 — `nil` when the author has no lud16/lud06 OR an older
            // kernel snapshot pre-dates the field. Mirrors the
            // forward/backward-compat pattern below (isRepost et al.).
            authorLnurl: try c.decodeIfPresent(String.self, forKey: .authorLnurl),
            kind: try c.decode(UInt32.self, forKey: .kind),
            content: try c.decode(String.self, forKey: .content),
            contentPreview: try c.decode(String.self, forKey: .contentPreview),
            createdAtDisplay: try c.decode(String.self, forKey: .createdAtDisplay),
            relayCount: try c.decode(UInt32.self, forKey: .relayCount),
            isRepost: try c.decodeIfPresent(Bool.self, forKey: .isRepost) ?? false,
            navTargetId: try c.decodeIfPresent(String.self, forKey: .navTargetId) ?? id,
            repostInnerContent: try c.decodeIfPresent(String.self, forKey: .repostInnerContent) ?? ""
        )
    }
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
