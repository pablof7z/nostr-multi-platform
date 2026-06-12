import Darwin
import Foundation
import os.log

let kbLog = Logger(subsystem: "io.f7z.chirp", category: "KernelBridge")

/// Mirror of `KERNEL_SCHEMA_VERSION` (Rust: `crates/nmp-core/src/update_envelope.rs`).
/// Must be bumped in lock-step when the Rust constant changes. A mismatch causes
/// `KernelBridge.decode()` to reject the snapshot rather than silently misparse
/// renamed or retyped fields (see `update.rs` contract comment).
private let KERNEL_SCHEMA_VERSION: UInt32 = 1

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
        // get_public_key handshake on a worker thread, then translates the
        // broker's signer-ready event into
        // `AddSigner(source: RemoteHandle, make_active:)`.
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

    /// Wire the Rust update callback. `handler` runs on every snapshot frame.
    /// Snapshot updates are binary-only FlatBuffers `nmp.transport.UpdateFrame`
    /// bytes. There is no runtime JSON fallback path.
    func listen(
        _ handler: @escaping (KernelUpdateResult) -> Void,
        onPanic: @escaping () -> Void = {}
    ) {
        let sink = KernelUpdateSink(handler: handler, onPanic: onPanic)
        updateSink = sink
        nmp_app_set_update_callback(
            raw,
            Unmanaged.passUnretained(sink).toOpaque(),
            nmpUpdateCallback)
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
        pubkey.withCString { nmp_app_chirp_open_author_feed(raw, $0) }
    }

    func openThread(eventID: String) {
        eventID.withCString { nmp_app_chirp_open_thread_feed(raw, $0) }
    }

    // M2 (ADR-0042): `openFirehose(tag:)` and the `nmp_app_open_firehose_tag`
    // C symbol it wrapped were deleted. A hashtag feed is now expressed through
    // `openInterest` below with a `{"kinds":[1],"#t":["<tag>"]}` filter at
    // `.global` scope — the kind set + `#t` filter that the firehose verb
    // hardcoded in the substrate now live app-side (D0-correct).

    /// M2 (ADR-0042) — generic feed-subscription open. `filterJSON` is a
    /// verbatim NIP-01 REQ filter (the app owns the kind set, e.g.
    /// `{"kinds":[1,6],"authors":["<hex>"]}`); `consumerID` refcounts owners so
    /// repeated opens of the same filter share one live subscription; `scope`
    /// is `.activeAccount` (re-route on switch) or `.global` (account-agnostic).
    /// Generic replacement for the deleted `openFirehose`. V-112 (ADR-0042):
    /// `openAuthor` / `openThread` now delegate to the chirp feed seam below.
    func openInterest(filterJSON: String, consumerID: String, scope: InterestScope) {
        filterJSON.withCString { filterPtr in
            consumerID.withCString { consumerPtr in
                nmp_app_open_interest(raw, filterPtr, consumerPtr, scope.rawValue)
            }
        }
    }

    /// M2 (ADR-0042) — detach one owner from a feed interest opened with
    /// `openInterest`. The live subscription is dropped on the last owner's
    /// close. Pass the SAME `filterJSON` / `consumerID` / `scope` the open used.
    func closeInterest(filterJSON: String, consumerID: String, scope: InterestScope) {
        filterJSON.withCString { filterPtr in
            consumerID.withCString { consumerPtr in
                nmp_app_close_interest(raw, filterPtr, consumerPtr, scope.rawValue)
            }
        }
    }

    /// F-TTL — `force` controls the lazy re-verification gate for the cached
    /// kind:0 profile. Pass `true` only when the user explicitly opened this
    /// author's profile screen or pulled to refresh; default `false` is the
    /// lazy, TTL-gated path for background / `.onAppear` component self-claims.
    func claimProfile(pubkey: String, consumerID: String, force: Bool = false) {
        pubkey.withCString { pkPtr in
            consumerID.withCString { cidPtr in
                nmp_app_claim_profile(raw, pkPtr, cidPtr, force ? 1 : 0)
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

    /// ADR-0032 / V-115: bech32-encode a hex pubkey as `npub1…` on the shell
    /// side. Projections no longer carry pre-encoded npub strings; shells call
    /// this when they need the bech32 form (copy-to-clipboard, share sheet).
    /// Returns `nil` if the C function fails (e.g. invalid key).
    func encodeProfile(pubkey: String) -> String? {
        pubkey.withCString { pkPtr -> String? in
            guard let ptr = nmp_app_encode_profile(raw, pkPtr) else { return nil }
            defer { nmp_app_free_string(ptr) }
            return String(cString: ptr)
        }
    }

    /// F-TTL — `force` controls the lazy re-verification gate; it only has an
    /// effect for `naddr` (addressable / replaceable) URIs and is a silent
    /// no-op for immutable `nevent`/`note` URIs. Pass `true` only when the
    /// user explicitly navigated to / opened this article/event or pulled to
    /// refresh; default `false` is the background path.
    func claimEvent(uri: String, consumerID: String, force: Bool = false) {
        uri.withCString { uriPtr in
            consumerID.withCString { cidPtr in
                nmp_app_claim_event(raw, uriPtr, cidPtr, force ? 1 : 0)
            }
        }
    }

    func releaseEvent(uri: String, consumerID: String) {
        uri.withCString { uriPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_event(raw, uriPtr, cidPtr)
            }
        }
    }

    /// Signal that the author feed for `pubkey` is no longer visible.
    /// Tears down the author-subscription so the kernel's wire_subs count
    /// returns to baseline. Call from `.onDisappear` on the AuthorView
    /// (ProfileView) to prevent sub-leaks on navigation pop.
    func closeAuthor(pubkey: String) {
        pubkey.withCString { nmp_app_chirp_close_author_feed(raw, $0) }
    }

    /// Signal that the thread for `eventID` is no longer visible.
    /// Symmetric counterpart to `openThread`; call from `.onDisappear`
    /// on the ThreadScreen to release the thread subscription.
    func closeThread(eventID: String) {
        eventID.withCString { nmp_app_chirp_close_thread_feed(raw, $0) }
    }

    // ── T66a identity / publish / multi-account / relay-edit ──────────────

    // NOTE: the local-nsec sign-in path does NOT go through `nmp_app_signin_nsec`
    // here — `KernelModel.addSigner(localNsec:)` routes through
    // `MarmotBridge.signInNsecAndRegisterMarmot` (the Chirp/Marmot identity FFI)
    // so the MLS registration side-effect is preserved. The bare
    // `nmp_app_signin_nsec` wrapper that used to live here had no callers and was
    // removed when the Rust `add_signer` redesign landed.

    // Compatibility C ABI: the stable `nmp_app_signin_bunker` symbol now routes
    // through Rust's unified `AddSigner { source: BunkerUri, .. }` command.
    // Swift keeps the old symbol name so shipped shells do not need a header
    // churn for the internal actor-command rename.
    func signInBunker(_ uri: String) {
        uri.withCString { nmp_app_signin_bunker(raw, $0, 1) }
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
                nmp_app_create_new_account(raw, profilePtr, relaysPtr, mls, 1)
            }
        }
        return nil
    }

    /// Publish a kind:0 profile metadata event for the active account through
    /// the kernel's `ActionModule` family. Routes via the single
    /// namespace-keyed `nmp_app_dispatch_action` entry point. Swift supplies
    /// profile fields only; Rust builds the action JSON, kind:0 event,
    /// `created_at` stamp, and signature. PR-A: returns the synchronous
    /// dispatch result so the caller can drive a spinner keyed on the
    /// correlation_id (or surface the error envelope to the user).
    @discardableResult
    func publishProfile(name: String, about: String, picture: String) -> DispatchResult {
        dispatchChirpIntent(.publishProfile(name: name, about: about, picture: picture))
    }

    func switchActive(identityID: String) {
        identityID.withCString { nmp_app_switch_active(raw, $0) }
    }

    func removeAccount(identityID: String) {
        identityID.withCString { nmp_app_remove_account(raw, $0) }
    }

    /// Publish a kind:1 note (optionally a reply) through the kernel's
    /// `ActionModule` family. Swift supplies compose input only; Rust builds
    /// the `nmp.publish` action spec, `PublishRaw` body, and any NIP-10 tags.
    /// PR-A: returns the synchronous dispatch result so the caller can drive a
    /// spinner keyed on the correlation_id (or surface the error envelope to the
    /// user). The terminal verdict arrives through
    /// `projections["action_results"]` on a later snapshot tick — match by
    /// `correlation_id` to clear the spinner.
    ///
    @discardableResult
    func publishNote(content: String, replyTo: ChirpReplyTarget?) -> DispatchResult {
        dispatchChirpIntent(.publishNote(content: content, replyTo: replyTo))
    }

    /// Publish a kind:6 repost of the given note through `PublishRaw`.
    /// NIP-18: tags `["e", eventID]` and `["p", authorPubkey]`, empty content.
    @discardableResult
    func repost(eventID: String, authorPubkey: String) -> DispatchResult {
        dispatchChirpIntent(.repost(eventID: eventID, authorPubkey: authorPubkey))
    }

    func retryPublish(handle: String) {
        handle.withCString { nmp_app_retry_publish(raw, $0) }
    }

    func cancelPublish(handle: String) {
        handle.withCString { nmp_app_cancel_publish(raw, $0) }
    }

    /// Dispatch an already-authored JSON action through the generic
    /// `nmp_app_dispatch_action` path. Common Chirp social/write actions use
    /// `dispatchChirpIntent` so Rust owns the protocol envelope; this helper
    /// remains for existing Rust-authored specs and action families that still
    /// accept a small host DTO.
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
        dispatchChirpIntent(.react(eventID: targetEventID, reaction: reaction))
    }

    @discardableResult
    func follow(pubkey: String) -> DispatchResult {
        dispatchChirpIntent(.follow(pubkey: pubkey))
    }

    @discardableResult
    func unfollow(pubkey: String) -> DispatchResult {
        dispatchChirpIntent(.unfollow(pubkey: pubkey))
    }

    /// Dispatch a NIP-57 zap through the `nmp.nip57.zap` ActionModule.
    /// Rust signs the kind:9734 zap request, completes the two-leg LNURL-pay
    /// round-trip, and (when the `wallet` feature is active) auto-dispatches
    /// `ActorCommand::WalletPayInvoice` so the bolt11 → NWC pay loop closes
    /// without a second host round-trip. The shell never sees the bolt11
    /// or parses LNURL/kind:9734 — thin-shell rule (aim.md §6.9).
    ///
    /// `lnurl` is the pre-extracted `authorLnurl` from the timeline item.
    /// Relay selection stays kernel policy. PR-A: returns the
    /// synchronous dispatch envelope so the host can drive a spinner keyed
    /// on the minted correlation_id.
    @discardableResult
    func zap(
        targetEventID: String,
        authorPubkey: String,
        lnurl: String,
        amountMsats: UInt64,
        comment: String? = nil
    ) -> DispatchResult {
        dispatchChirpIntent(.zap(
            targetEventID: targetEventID,
            recipientPubkey: authorPubkey,
            amountMsats: amountMsats,
            lnurl: lnurl,
            comment: comment
        ))
    }

    /// Build and dispatch a Chirp action spec authored by Rust.
    ///
    /// Swift owns only raw user intent. Rust returns the exact namespace and
    /// body JSON to feed through `nmp_app_dispatch_action`.
    @discardableResult
    func dispatchChirpIntent(_ intent: ChirpActionIntent) -> DispatchResult {
        let intentJson: String
        do {
            let data = try JSONEncoder().encode(intent)
            guard let json = String(data: data, encoding: .utf8) else {
                return .failure("failed to encode Chirp action intent as UTF-8")
            }
            intentJson = json
        } catch {
            return .failure("failed to encode Chirp action intent: \(error.localizedDescription)")
        }
        let specJson: String? = intentJson.withCString { intentPtr in
            guard let ptr = nmp_app_chirp_action_spec(intentPtr) else {
                return nil
            }
            defer { nmp_app_free_string(ptr) }
            return String(cString: ptr)
        }
        guard let specJson else {
            return .failure("action spec builder returned a null envelope")
        }
        let spec: ChirpActionSpecEnvelope
        do {
            guard let data = specJson.data(using: .utf8) else {
                return .failure("action spec envelope was not UTF-8")
            }
            spec = try JSONDecoder().decode(ChirpActionSpecEnvelope.self, from: data)
        } catch {
            return .failure("failed to decode action spec envelope: \(error.localizedDescription)")
        }
        if let error = spec.error {
            return .failure(error)
        }
        guard let namespace = spec.namespace, let bodyJson = spec.bodyJson,
              !namespace.isEmpty, !bodyJson.isEmpty else {
            return .failure("action spec envelope missing dispatch fields")
        }
        return dispatchRawAction(namespace: namespace, bodyJson: bodyJson)
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

    /// `nmp.nip65.publish_relay_list` — dispatches a kind:10002 NIP-65
    /// relay-list metadata event. Swift forwards the kernel-authored
    /// `AppRelay` role string verbatim; Rust normalizes composite roles
    /// like `"both,indexer"` and skips indexer-only rows when building the
    /// kind:10002 tags.
    @discardableResult
    func publishRelayList(relays: [AppRelay]) -> DispatchResult {
        return dispatchAction(
            namespace: "nmp.nip65.publish_relay_list",
            body: ["relays": relays.map { ["url": $0.url, "role": $0.role] }])
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

    fileprivate static func decodeFlatBuffer(bytes: UnsafeRawPointer, count: Int) -> KernelDecodedUpdateFrame? {
        let start = ContinuousClock.now
        let data = Data(bytes: bytes, count: count)
        do {
            let frame = try KernelUpdateFrameDecoder.decode(data)
            guard case let .snapshot(frameSchemaVersion, envelopes, flatFeeds, typedEnvelope) = frame else {
                if case let .panic(message) = frame {
                    kbLog.fault("NMP_ACTOR_PANIC detected bytes=\(data.count) msg=\(message, privacy: .public)")
                    return .panic(message)
                }
                return nil
            }
            // Enforce the schema version contract: a mismatch means Rust's
            // field layout changed in a way the host cannot safely interpret.
            // Return nil so the update is dropped rather than misparsed. The
            // generic `payload` is no longer decoded, so the frame-level
            // `schema_version` is the sole gate (it mirrors the former
            // payload-level check — both were written from the same value).
            guard frameSchemaVersion == KERNEL_SCHEMA_VERSION else {
                kbLog.error("schema version mismatch: frame=\(frameSchemaVersion) host=\(KERNEL_SCHEMA_VERSION) — snapshot rejected")
                return nil
            }
            // ADR-0038 typed path: prefer the typed home-feed decode when the
            // NOFS sidecar is present and fully decodable (NFCT bytes filled).
            // Returns nil when absent or malformed → generic path stays active
            // (ADR-0037 Commitment 4 graceful fallback).
            let typedHomeFeed = TypedHomeFeedDecoder.decode(from: envelopes)
            // V6 Stage 4 (Wave B): prefer the typed `accounts` / `active_account`
            // sidecars when present and well-formed. Each returns nil when the
            // sidecar is absent or malformed → the generic `projections.<field>`
            // JSON path stays active (ADR-0037 Commitment 4 graceful fallback),
            // exactly mirroring `typedHomeFeed` above.
            let typedAccounts = TypedAccountsDecoder.decode(from: envelopes)
            let typedActiveAccount = TypedActiveAccountDecoder.decode(from: envelopes)
            // V6 Stage 4 (Wave B batch #2): the relay-settings + publish-cluster
            // thin-glue keys. Each returns nil when its sidecar is absent or
            // malformed → the generic `projections.<field>` JSON path stays
            // active (ADR-0037 Commitment 4), mirroring `typedAccounts` above.
            let typedConfiguredRelays = TypedConfiguredRelaysDecoder.decode(from: envelopes)
            let typedRelayRoleOptions = TypedRelayRoleOptionsDecoder.decode(from: envelopes)
            let typedOutboxSummary = TypedOutboxSummaryDecoder.decode(from: envelopes)
            let typedPublishOutbox = TypedPublishOutboxDecoder.decode(from: envelopes)
            let typedPublishQueue = TypedPublishQueueDecoder.decode(from: envelopes)
            // V6 Stage 4 (Wave B batch #3): the diagnostics + action-lifecycle
            // keys. Each returns nil when its sidecar is absent or malformed →
            // the generic `projections.<field>` JSON path stays active
            // (ADR-0037 Commitment 4), mirroring `typedAccounts` above.
            let typedRelayDiagnostics = TypedRelayDiagnosticsDecoder.decode(from: envelopes)
            let typedActionLifecycle = TypedActionLifecycleDecoder.decode(from: envelopes)
            // V6 Stage 4 (Wave B Tier-1 #4): the app-projection keys
            // (`nmp.follow_list` / `nmp.nip57.zaps` / `nmp.nip29.group_chat` /
            // `nmp.nip29.discovered_groups`). Each returns nil when its sidecar is
            // absent or malformed → the generic `projections.<field>` JSON path
            // stays active (ADR-0037 Commitment 4), mirroring `typedAccounts`
            // above. `nmp.follow_list`'s envelope KEY (`nmp.follow_list`) differs
            // from its payload SCHEMA_ID (`nmp.nip02.follow_list`); the generated
            // decoder matches on both.
            let typedFollowList = TypedFollowListDecoder.decode(from: envelopes)
            let typedZaps = TypedZapsDecoder.decode(from: envelopes)
            let typedGroupChat = TypedGroupChatDecoder.decode(from: envelopes)
            let typedDiscoveredGroups = TypedDiscoveredGroupsDecoder.decode(from: envelopes)
            // Profile-cluster typed sidecars (`profile` / `claimed_profiles` /
            // `resolved_profiles`). All three share the `nmp_kernel_ProfileCard`
            // reader (defined once in `ProfileCard.generated.swift`). Each returns
            // nil when its sidecar is absent/malformed → the generic
            // `projections.<field>` JSON path stays active (ADR-0037 Commitment 4),
            // mirroring `typedAccounts` above.
            let typedProfile = TypedProfileDecoder.decode(from: envelopes)
            let typedClaimedProfiles = TypedClaimedProfilesDecoder.decode(from: envelopes)
            let typedResolvedProfiles = TypedResolvedProfilesDecoder.decode(from: envelopes)
            // NIP-17 DM cluster + claimed-event map (`nmp.nip17.dm_inbox` /
            // `nmp.nip17.dm_relay_list` / `claimed_events`). Each returns nil when
            // its sidecar is absent/malformed → the generic `projections.<field>`
            // JSON path stays active (ADR-0037 Commitment 4), mirroring
            // `typedAccounts` above. `dm_relay_list` has no Swift read consumer
            // yet — the decode is wired for parity and unit-tested.
            let typedDmInbox = TypedDmInboxDecoder.decode(from: envelopes)
            let typedDmRelayList = TypedDmRelayListDecoder.decode(from: envelopes)
            let typedClaimedEvents = TypedClaimedEventsDecoder.decode(from: envelopes)
            // NIP-46 cluster (`bunker_handshake` / `nip46_onboarding`). Each
            // returns nil when its sidecar is absent/malformed → the generic
            // `projections.<field>` JSON path stays active (ADR-0037 Commitment
            // 4), mirroring `typedAccounts` above. `bunker_handshake`'s typed
            // closure emits NO sidecar while idle (slot is `None`), so nil there
            // is the steady-state — the generic JSON `null` is the fallback.
            let typedBunkerHandshake = TypedBunkerHandshakeDecoder.decode(from: envelopes)
            let typedNip46Onboarding = TypedNip46OnboardingDecoder.decode(from: envelopes)
            // V-14: relay-layer bunker connection health (`bunker_connection_state`,
            // KBCS). Nil while no bunker session is active (slot is `None`) — the
            // steady state for local-key accounts. `isConnected`/`isReconnecting`/
            // `isFailed` drive status badges; no generic JSON fallback needed
            // because iOS has always needed the sidecar (ADR-0037 §4).
            let typedBunkerConnectionState = TypedBunkerConnectionStateDecoder.decode(from: envelopes)
            // Marmot push-projection cluster (`nmp.marmot.snapshot` /
            // `nmp.marmot.messages`, V-107 / ADR-0039). Each returns nil when its
            // sidecar is absent/malformed → the generic `projections.<field>` JSON
            // path stays active (ADR-0037 Commitment 4), mirroring `typedAccounts`
            // above. The typed closures emit NO sidecar while signed-out (slot is
            // `None`), so nil there is the steady-state — the generic JSON
            // empty-object fallback applies and `MarmotStore.apply` maps it to
            // `.empty` / `[:]`.
            let typedMarmotSnapshot = TypedMarmotSnapshotDecoder.decode(from: envelopes)
            let typedMarmotMessages = TypedMarmotMessagesDecoder.decode(from: envelopes)
            // `wallet` (NWST, producer field-add) + `settings_hub` (KSHB, kernel
            // built-in). Each returns nil when its sidecar is absent/malformed →
            // the generic `projections.<field>` JSON path stays active (ADR-0037
            // Commitment 4), mirroring `typedAccounts` above. The wallet typed
            // closure emits NO sidecar while disconnected (slot is `None`), so nil
            // there is the steady-state — the generic JSON `null` fallback applies.
            let typedWallet = TypedWalletDecoder.decode(from: envelopes)
            let typedSettingsHub = TypedSettingsHubDecoder.decode(from: envelopes)
            // Wave C: action_results, action_stages.
            // V-112 (ADR-0042): author_view / thread_view typed sidecars deleted.
            let typedActionResults = TypedActionResultsDecoder.decode(from: envelopes)
            let typedActionStages = TypedActionStagesDecoder.decode(from: envelopes)
            let duration = start.duration(to: .now)
            kbLog.info("decoded ok rev=\(typedEnvelope?.rev ?? 0) activeAccount=\(typedActiveAccount ?? "nil")")
            return .snapshot(
                KernelUpdateResult(
                    typedHomeFeed: typedHomeFeed,
                    typedAccounts: typedAccounts,
                    typedActiveAccount: typedActiveAccount,
                    typedConfiguredRelays: typedConfiguredRelays,
                    typedRelayRoleOptions: typedRelayRoleOptions,
                    typedOutboxSummary: typedOutboxSummary,
                    typedPublishOutbox: typedPublishOutbox,
                    typedPublishQueue: typedPublishQueue,
                    typedRelayDiagnostics: typedRelayDiagnostics,
                    typedActionLifecycle: typedActionLifecycle,
                    typedFollowList: typedFollowList,
                    typedZaps: typedZaps,
                    typedGroupChat: typedGroupChat,
                    typedDiscoveredGroups: typedDiscoveredGroups,
                    typedProfile: typedProfile,
                    typedClaimedProfiles: typedClaimedProfiles,
                    typedResolvedProfiles: typedResolvedProfiles,
                    typedDmInbox: typedDmInbox,
                    typedDmRelayList: typedDmRelayList,
                    typedClaimedEvents: typedClaimedEvents,
                    typedBunkerHandshake: typedBunkerHandshake,
                    typedNip46Onboarding: typedNip46Onboarding,
                    typedBunkerConnectionState: typedBunkerConnectionState,
                    typedMarmotSnapshot: typedMarmotSnapshot,
                    typedMarmotMessages: typedMarmotMessages,
                    typedWallet: typedWallet,
                    typedSettingsHub: typedSettingsHub,
                    typedActionResults: typedActionResults,
                    typedActionStages: typedActionStages,
                    // V-112 (ADR-0042): typedAuthorView / typedThreadView removed.
                    typedEnvelope: typedEnvelope,
                    flatFeeds: flatFeeds,
                    payloadBytes: data.count,
                    callbackReceivedAt: start,
                    decodeMicros: duration.microseconds
                )
            )
        } catch let error as DecodingError {
            switch error {
            case let .keyNotFound(key, ctx):
                kbLog.error("FlatBuffers decode: keyNotFound '\(key.stringValue)' at \(ctx.codingPath.map(\.stringValue).joined(separator: ".")) bytes=\(data.count)")
            case let .typeMismatch(_, ctx):
                kbLog.error("FlatBuffers decode: typeMismatch at \(ctx.codingPath.map(\.stringValue).joined(separator: ".")) — \(ctx.debugDescription) bytes=\(data.count)")
            default:
                kbLog.error("FlatBuffers decode error: \(error.localizedDescription) bytes=\(data.count)")
            }
            return nil
        } catch {
            kbLog.error("FlatBuffers snapshot decode error: \(error.localizedDescription) bytes=\(data.count)")
            return nil
        }
    }
}

private final class KernelUpdateSink {
    let handler: (KernelUpdateResult) -> Void
    /// D7 actor-death hook. Rust emits a FlatBuffers panic frame before the
    /// update channel closes; the host flips its fatal-error UI from here.
    let onPanic: () -> Void

    init(
        handler: @escaping (KernelUpdateResult) -> Void,
        onPanic: @escaping () -> Void
    ) {
        self.handler = handler
        self.onPanic = onPanic
    }
}

enum KernelDecodedUpdateFrame {
    case snapshot(KernelUpdateResult)
    case panic(String)
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

private let nmpUpdateCallback: NmpUpdateCallback = { context, bytes, count in
    guard let context, let bytes, count > 0 else { return }
    let sink = Unmanaged<KernelUpdateSink>.fromOpaque(context).takeUnretainedValue()
    guard let frame = KernelHandle.decodeFlatBuffer(bytes: UnsafeRawPointer(bytes), count: Int(count)) else {
        return
    }
    switch frame {
    case let .snapshot(result):
        sink.handler(result)
    case .panic:
        sink.onPanic()
    }
}

// ─── Typed SnapshotFrame envelope (ADR-0044 Tier-3) ───────────────────────

/// The typed `SnapshotFrame` envelope fields, read DIRECTLY off the
/// `SnapshotFrame` table (ADR-0044) — distinct from the `typed_projections`
/// sidecar list every other `typed*` decode walks. PR #1034 added these
/// first-class fields (`rev`, `running`, `metrics`, the relay/interest/wire
/// vectors, `logs`) on the frame so a migrated host reads them instead of
/// re-walking the generic JSON `payload` tree.
///
/// All seven fields are written by the producer as a UNIT
/// (`encode_snapshot_with_envelope`, `kernel/update.rs`) whenever the frame
/// carries metrics, so this whole struct is gated on the one field whose
/// FlatBuffers accessor reports presence (`SnapshotFrame.metrics != nil`). When
/// the gate is open the host prefers these typed values; when it is closed (a
/// legacy frame, or the test-only `encode_snapshot_with_typed` path) the value
/// is `nil` and every accessor falls through to the generic JSON `payload`
/// (`snapshot?.<field>`) — ADR-0037 Commitment 4. Every value is a raw mirror
/// of the top-level `KernelSnapshot` fields (ADR-0032), field-identical to the
/// JSON decode. This is the LAST consumer of the generic `payload`'s top-level
/// scalars.
struct TypedSnapshotEnvelope: Equatable {
    let rev: UInt64
    let running: Bool
    let metrics: KernelMetrics
    let relayStatuses: [RelayStatus]
    let logicalInterests: [LogicalInterestStatus]
    let wireSubscriptions: [WireSubscriptionStatus]
    let logs: [String]
    /// Snapshot-driven error toast — read DIRECTLY off the `SnapshotFrame`
    /// table (`last_error_toast`), the same first-class envelope tier as the
    /// other fields. `nil` ⇒ no toast on this tick. This re-homes the last
    /// raw whole-payload read (`update.lastErrorToast`) onto the typed
    /// envelope; `KernelModel` copies it into its user-clearable
    /// `lastErrorToast` slot in `apply(result:)`.
    let lastErrorToast: String?
}

// ─── Swift-side timing wrapper ────────────────────────────────────────────

struct KernelUpdateResult {
    /// Typed home-feed decode result (ADR-0038 typed path). Non-nil when the
    /// snapshot carried a well-formed `NOFS` typed projection that the Swift
    /// `NFCT` decoder could fully populate. `nil` means the generic
    /// `projections.homeFeed` fallback applies (ADR-0037 Commitment 4).
    let typedHomeFeed: ChirpTimelineSnapshot?
    /// Typed `accounts` projection decode (V6 Stage 4 / Wave B `KACC` sidecar).
    /// Non-nil when the snapshot carried a well-formed `accounts` typed sidecar;
    /// `nil` means the generic `projections.accounts` JSON fallback applies.
    let typedAccounts: [AccountSummary]?
    /// Typed `active_account` projection decode (V6 Stage 4 / Wave B `KACT`
    /// sidecar). Non-nil when the snapshot carried a well-formed `active_account`
    /// typed sidecar that resolved to an active pubkey; `nil` means either no
    /// sidecar OR no active account — both defer to the generic
    /// `projections.active_account` JSON fallback (parity-preserving).
    let typedActiveAccount: String?
    /// Typed `configured_relays` projection decode (V6 Stage 4 / Wave B `KCRL`
    /// sidecar). `nil` ⇒ the generic `projections.configured_relays` JSON
    /// fallback applies.
    let typedConfiguredRelays: [AppRelay]?
    /// Typed `relay_role_options` projection decode (`KRRO`). `nil` ⇒ generic
    /// `projections.relay_role_options` JSON fallback.
    let typedRelayRoleOptions: [RelayRoleOption]?
    /// Typed `outbox_summary` projection decode (`KOXS`). `nil` ⇒ generic
    /// `projections.outbox_summary` JSON fallback.
    let typedOutboxSummary: OutboxSummary?
    /// Typed `publish_outbox` projection decode (`KPBO`). `nil` ⇒ generic
    /// `projections.publish_outbox` JSON fallback.
    let typedPublishOutbox: [PublishOutboxItem]?
    /// Typed `publish_queue` projection decode (`KPBQ`). The domain type is a
    /// field-subset of the wire. `nil` ⇒ generic `projections.publish_queue`
    /// JSON fallback.
    let typedPublishQueue: [PublishQueueEntry]?
    /// Typed `relay_diagnostics` projection decode (`KRDG`). `nil` ⇒ generic
    /// `projections.relay_diagnostics` JSON fallback.
    let typedRelayDiagnostics: RelayDiagnosticsSnapshot?
    /// Typed `action_lifecycle` projection decode (`KALC`). `nil` ⇒ generic
    /// `projections.action_lifecycle` JSON fallback.
    let typedActionLifecycle: ActionLifecycleSnapshot?
    /// Typed `nmp.follow_list` projection decode (`NF02`; envelope key
    /// `nmp.follow_list`, schema_id `nmp.nip02.follow_list`). `nil` ⇒ generic
    /// `projections["nmp.follow_list"]` JSON fallback.
    let typedFollowList: FollowListSnapshot?
    /// Typed `nmp.nip57.zaps` projection decode (`NZAP`). `nil` ⇒ generic
    /// `projections["nmp.nip57.zaps"]` JSON fallback.
    let typedZaps: ZapsAggregateSnapshot?
    /// Typed `nmp.nip29.group_chat` projection decode (`NGCS`). `nil` ⇒ generic
    /// `projections["nmp.nip29.group_chat"]` JSON fallback.
    let typedGroupChat: GroupChatSnapshot?
    /// Typed `nmp.nip29.discovered_groups` projection decode (`NDGS`). `nil` ⇒
    /// generic `projections["nmp.nip29.discovered_groups"]` JSON fallback.
    let typedDiscoveredGroups: DiscoveredGroupsSnapshot?
    /// Typed `profile` projection decode (`KPRF`). `nil` ⇒ generic
    /// `projections["profile"]` JSON fallback.
    let typedProfile: ProfileCard?
    /// Typed `claimed_profiles` projection decode (`KCPR`). `nil` ⇒ generic
    /// `projections["claimed_profiles"]` JSON fallback.
    let typedClaimedProfiles: [String: ProfileCard]?
    /// Typed `resolved_profiles` projection decode (`KRPR`). `nil` ⇒ generic
    /// `projections["resolved_profiles"]` JSON fallback.
    let typedResolvedProfiles: [String: ProfileCard]?
    /// Typed `nmp.nip17.dm_inbox` projection decode (`NDMI`). `nil` ⇒ generic
    /// `projections["nmp.nip17.dm_inbox"]` JSON fallback. Routed to the
    /// `dmInbox` store (typed-first effective value) in `KernelModel.apply`.
    let typedDmInbox: DmInboxSnapshot?
    /// Typed `nmp.nip17.dm_relay_list` projection decode (`NDRL`). `nil` ⇒ generic
    /// `projections["nmp.nip17.dm_relay_list"]` JSON fallback. No Swift read
    /// consumer yet — read through the `dmRelayList` accessor (added for parity).
    let typedDmRelayList: DmRelayListSnapshot?
    /// Typed `claimed_events` projection decode (`KCEV`). `nil` ⇒ generic
    /// `projections.claimedEvents` JSON fallback. Routed to `EmbedHost.update`
    /// (typed-first effective value) in `KernelModel.apply`.
    let typedClaimedEvents: [String: ClaimedEventDto]?
    /// Typed `bunker_handshake` projection decode (`KBHS`). `nil` ⇒ generic
    /// `projections["bunker_handshake"]` JSON fallback. The producer emits no
    /// sidecar while the handshake slot is idle, so nil is the steady state.
    let typedBunkerHandshake: BunkerHandshake?
    /// Typed `nip46_onboarding` projection decode (`KN46`). `nil` ⇒ generic
    /// `projections["nip46_onboarding"]` JSON fallback. Always present from a
    /// current kernel (the static signer-app table is emitted every tick).
    let typedNip46Onboarding: Nip46Onboarding?
    /// Typed `bunker_connection_state` projection decode (`KBCS`). V-14 / #963.
    /// `nil` while no bunker session is active (relay socket not yet opened).
    /// `nil` is the steady state for local-key accounts — no JSON fallback
    /// available because iOS is typed-sidecar-only (ADR-0037 §4). When non-nil,
    /// `isConnected` drives the green dot, `isReconnecting` the amber badge, and
    /// `isFailed` the red re-auth prompt (ADR-0032 / relay_diagnostics pattern).
    let typedBunkerConnectionState: BunkerConnectionState?
    /// Typed `nmp.marmot.snapshot` projection decode (`NMMS`, V-107 / ADR-0039).
    /// `nil` ⇒ generic `projections["nmp.marmot.snapshot"]` JSON fallback. Routed
    /// to `MarmotStore.apply` (typed-first effective value) in `KernelModel.apply`.
    /// The producer emits no sidecar while signed-out, so nil is the steady state.
    let typedMarmotSnapshot: MarmotSnapshot?
    /// Typed `nmp.marmot.messages` projection decode (`NMMG`, V-107 / ADR-0039).
    /// `nil` ⇒ generic `projections["nmp.marmot.messages"]` JSON fallback. The
    /// flattened-vector wire rebuilds the `group_id_hex -> [MarmotMessage]` map.
    /// Routed to `MarmotStore.apply` (typed-first effective value) in
    /// `KernelModel.apply`.
    let typedMarmotMessages: [String: [MarmotMessage]]?
    /// Typed `wallet` projection decode (`NWST`). `nil` ⇒ generic
    /// `projections["wallet"]` JSON fallback. Read typed-first through the
    /// `walletStatus` accessor (`typedWallet ?? snapshot?.walletStatus`) in
    /// `KernelModel+Projections`. The producer emits no sidecar while the wallet
    /// is disconnected (slot is `None`), so nil is the steady state. The
    /// `wallet_pubkey_hex` producer field-add unblocked this flip.
    let typedWallet: WalletStatusData?
    /// Typed `settings_hub` projection decode (`KSHB`, kernel built-in). `nil` ⇒
    /// generic `projections["settings_hub"]` JSON fallback. The single-key
    /// `["relay_count": Int]` dict is read typed-first through the `settingsHub`
    /// accessor in `KernelModel+Projections` and wrapped into `SettingsHubSummary`.
    let typedSettingsHub: [String: Int]?
    /// Wave C: Typed `action_results` projection decode (`KARS`). `nil` ⇒ generic
    /// `projections.action_results` JSON fallback. The per-tick drain array; maps
    /// each `ActionResult` row to `LastActionResult`. NOTE: no read site wired yet
    /// (foundation only; wire typed-first in `KernelModel.apply` as follow-up).
    let typedActionResults: [LastActionResult]?
    /// Wave C: Typed `action_stages` projection decode (`KAST`). `nil` ⇒ generic
    /// `projections.action_stages` JSON fallback. The flat-vector wire rebuilds
    /// the `[correlation_id: [ActionStageEntry]]` dictionary. NOTE: no read site
    /// wired yet (foundation only; wire typed-first in `KernelModel.apply` as
    /// follow-up).
    let typedActionStages: [String: [ActionStageEntry]]?
    // V-112 (ADR-0042): typedAuthorView (AuthorProfileSnapshot) and
    // typedThreadView (ThreadView) deleted — author_view / thread_view typed
    // sidecars removed with AuthorViewState / ThreadViewState.
    /// ADR-0044 Tier-3: the typed `SnapshotFrame` envelope (`rev` / `running` /
    /// `metrics` / `relayStatuses` / `logicalInterests` / `wireSubscriptions` /
    /// `logs`), read directly off the `SnapshotFrame` table. Non-nil when the
    /// frame carried the typed envelope (gated on `metrics`); `nil` ⇒ the
    /// generic JSON `payload` top-level scalars apply (read through the
    /// `KernelModel+Projections` accessors).
    let typedEnvelope: TypedSnapshotEnvelope?
    /// Dynamic per-screen flat feeds keyed as `nmp.feed.author.<pubkey>` or
    /// `nmp.feed.thread.<event_id>`. These keys are opened per navigation
    /// target, so they cannot be codegen'd as fixed projection fields.
    let flatFeeds: [String: ChirpTimelineSnapshot]
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
    /// minted by `ActionRegistry::start`. V5: the kernel's
    /// `action_lifecycle` projection automatically surfaces this id under
    /// `inFlight` until the action settles, then under `recentTerminal`
    /// for a 3-second window. The host does NOT maintain its own pending
    /// set — read `model.actionLifecycle` to drive the UI.
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

// `SnapshotProjections` is generated — see
// `ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift`. The Rust
// source of truth is the `SNAPSHOT_PROJECTIONS` registry in
// `crates/nmp-codegen/src/swift_projections_registry.rs`; the renderer in
// `crates/nmp-codegen/src/swift.rs` emits the equivalent struct + `CodingKeys`
// enum. Regenerate via:
//
//   cargo run -p nmp-core --features codegen-schema \
//       --bin dump_projection_schemas \
//     | cargo run -p nmp-codegen -- gen swift
//
// The `codegen-drift` CI gate fails any PR whose generated file differs
// from a fresh run, so a new dotted-key projection added to the Rust
// registry without regenerating Swift cannot land on master.

// ─── resolved_profiles projection adapter ─────────────────────────────────
//
// `MentionProfile` is the rich, component-facing struct `NoteRenderContext`
// consumes. It is now built from a `ProfileCard` carried by the pre-merged
// `projections["resolved_profiles"]` map (PR #812) rather than from the older
// `mention_profiles` wire DTO. The component API (`[String: MentionProfile]`)
// is unchanged — only the source projection is broader and merged once in Rust.
// No Swift derives a `MentionProfile` from a `TimelineItem` anymore.

extension MentionProfile {
    /// Bridge from a resolved `ProfileCard`. `display` falls back to the
    /// abbreviated hex pubkey when no kind:0 has arrived (`ProfileCard
    /// .displayLabel`); avatar initials and tint colour are derived locally
    /// from the same inputs (`PubkeyFormatting.swift`). ADR-0032 — backend
    /// ships raw data, presentation layer formats.
    init(card: ProfileCard) {
        let display = card.displayLabel
        self.init(
            display: display,
            pictureUrl: card.pictureUrl,
            initials: display.displayInitials,
            colorHex: card.pubkey.pubkeyColorHex
        )
    }
}

/// Settings-hub view projection — `projections["settings_hub"]`. The kernel
/// now emits `relay_count` as an integer; the iOS shell computes the
/// pluralized subtitle locally. Decoded under `.convertFromSnakeCase`, so the
/// Rust `relay_count` JSON key matches the synthesized `relayCount` property
/// name directly.
struct SettingsHubSummary: Decodable, Equatable {
    let relayCount: Int

    var relaysSubtitle: String {
        switch relayCount {
        case 0: return "No relays configured"
        case 1: return "1 relay"
        default: return "\(relayCount) relays"
        }
    }

    static let empty = SettingsHubSummary(relayCount: 0)
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
/// (inherited by every nested type) maps the kernel's `"created_at"` /
/// `"created_at_display"` to `createdAt` / `createdAtDisplay` automatically.
/// An explicit enum would have to spell the post-transform name and is pure
/// surface area — omitted deliberately.
struct GroupChatMessage: Decodable, Identifiable, Equatable {
    let id: String
    /// Author Nostr pubkey, hex (64 chars). Presentation layer formats for
    /// display (ADR-0032).
    let pubkey: String
    let content: String
    /// Event `created_at` (Unix seconds). Presentation layer formats for
    /// display via `relativeTimeFromUnixSeconds` (ADR-0032).
    let createdAt: UInt64
    let kind: UInt32
}

/// The serialised read-model a group-chat screen consumes. `messages` is
/// ordered newest-first (`created_at` descending, ties broken by id) by the
/// Rust projection — Swift does not re-sort. Avatar / initials for the
/// group tile are derived by the presentation layer (ADR-0032).
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
    /// Pre-computed 2-char uppercase initials for the avatar tile (Rust:
    /// first 2 chars of `name` if non-empty, else of `group_id`,
    /// uppercased). Thin-shell V-24 — no Swift derivation.
    let initials: String
    /// Display name: `name` when non-empty, `groupId` as fallback. The
    /// shell renders this verbatim without a null-coalescing conditional
    /// (V-24).
    let displayName: String
    /// Pre-formatted accessibility subtitle, e.g.
    /// `"# Public · Open · 5 members"` / `"🔒 Private · Closed · 1 member"`.
    /// Visibility glyphs (`#` / `🔒`) and pluralization live in Rust
    /// (V-24).
    let subtitle: String

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
    /// Event `created_at` (Unix seconds). Presentation layer formats via
    /// `relativeTimeFromUnixSeconds` (ADR-0032).
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
/// ADR-0032: only the raw peer hex pubkey crosses the FFI boundary. The
/// presentation layer formats it for display (`shortHex`,
/// `pubkeyColorHex`, `displayInitials`).
struct DmConversation: Decodable, Identifiable, Equatable {
    /// The OTHER party in the thread (hex pubkey). Also the list identity.
    let peerPubkey: String
    let messages: [DmMessage]

    var id: String { peerPubkey }
}

// ─── NIP-02 follow list read model ───────────────────────────────────────────
//
// Mirror of `nmp-app-chirp`'s `FollowListProjection` — the shape it serialises
// under the snapshot key `"nmp.follow_list"`. Follow entries carry raw pubkeys;
// Swift formats compact labels/avatars from those raw fields (ADR-0032).

/// One entry in the active account's follow list. Only the raw hex
/// `pubkey` crosses the FFI boundary; the presentation layer formats
/// the abbreviated label / avatar tint / initials locally (ADR-0032).
struct FollowEntry: Decodable, Identifiable, Equatable {
    let pubkey: String
    var id: String { pubkey }
}

/// The serialised follow-list snapshot. `follows` is the active account's
/// NIP-02 kind:3 contact list; each entry carries the raw followee pubkey.
struct FollowListSnapshot: Decodable, Equatable {
    let follows: [FollowEntry]
    static let empty = FollowListSnapshot(follows: [])
}

/// The serialised read-model the DM screens consume. `conversations` is
/// ordered by most-recent message (newest thread first) by the Rust
/// projection — Swift does not re-sort.
struct DmInboxSnapshot: Decodable, Equatable {
    let conversations: [DmConversation]
    /// Set by Rust (V-08) when the active account uses a NIP-46 bunker that
    /// cannot unseal gift-wraps. The host should surface a message instead of
    /// an empty list. `false` when signed in with local keys or not signed in.
    var remoteSignerUnsupported: Bool

    static let empty = DmInboxSnapshot(conversations: [], remoteSignerUnsupported: false)

    // Custom init so `remoteSignerUnsupported` degrades to `false` when the
    // field is absent (older Rust build that predates V-08). The decoder uses
    // `.convertFromSnakeCase`, so `remote_signer_unsupported` → property name.
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        conversations = try c.decode([DmConversation].self, forKey: .conversations)
        remoteSignerUnsupported = try c.decodeIfPresent(Bool.self, forKey: .remoteSignerUnsupported) ?? false
    }

    init(conversations: [DmConversation], remoteSignerUnsupported: Bool = false) {
        self.conversations = conversations
        self.remoteSignerUnsupported = remoteSignerUnsupported
    }

    private enum CodingKeys: String, CodingKey {
        case conversations
        case remoteSignerUnsupported
    }
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

/// NIP-46 bunker relay-layer connection state — `projections["bunker_connection_state"]`.
///
/// Tracks the health of the relay socket that the established bunker session
/// rides on. Distinct from `BunkerHandshake` (which tracks the NIP-46
/// connect / get_public_key handshake progress). Nil when no bunker session is
/// active (the projection contributes JSON `null`).
///
/// Rust pre-computes every flag so shells never string-compare `state`
/// (aim.md §6 / AP1). `isConnected` drives the green indicator; `isReconnecting`
/// drives an amber reconnecting badge; `isFailed` drives a red re-auth prompt.
///
/// `Decodable` for the JSON fallback path; `Equatable` for `@Published` diffing
/// so SwiftUI re-renders only on real state changes.
struct BunkerConnectionState: Decodable, Equatable {
    /// `"connected"` | `"reconnecting"` | `"failed"`. Carried verbatim from
    /// `BunkerConnectionStateDto::state`. Prefer the typed flag fields below.
    let state: String
    /// Optional human-readable reason (error message on `reconnecting`/`failed`).
    let reason: String?
    /// `state == "connected"`. Green indicator.
    let isConnected: Bool
    /// `state == "reconnecting"` — transient flap, auto-reconnect in progress.
    /// Amber badge; do NOT prompt re-auth yet.
    let isReconnecting: Bool
    /// `state == "failed"` — permanent error, session bricked.
    /// Red badge; prompt re-auth.
    let isFailed: Bool
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
//
// `LogicalInterestStatus` and `WireSubscriptionStatus` moved to
// `Generated/KernelTypes.generated.swift` (V6 Stage 1, plan §6b). The Rust
// projection types in `nmp-core/src/kernel/types.rs` are now the single
// source of truth — Swift mirrors are emitted from `schemars` schemas.

// ─── Domain types shared across the UI ───────────────────────────────────

// V-112 (ADR-0042): `ThreadView` Decodable deleted — the `thread_view`
// projection (and its `threadView` field on the generated
// `SnapshotProjections`) was removed with the kernel author/thread view
// stack. Thread rendering reads the per-app FlatFeed
// (`nmp_app_chirp_open_thread_feed`).

// `AccountSummary` moved to `Generated/KernelTypes.generated.swift` (V6
// Stage 1, plan §6b). Rust source: `nmp-core/src/kernel/identity_state.rs`
// `AccountSummary`. Field docs live alongside the Rust definition.

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

    /// Memberwise initializer. The custom `init(from:)` above suppresses
    /// Swift's synthesized memberwise init, so the Wave C typed-sidecar glue
    /// (`TypedProjectionGlue.actionStages`) needs this explicit one to build a
    /// row from the `flatc --swift` reader struct.
    init(stage: ActionStage, atMs: UInt64) {
        self.stage = stage
        self.atMs = atMs
    }
}

// ─── V5 thin-shell: action_lifecycle projection wire types ──────────────
//
// The kernel's `action_lifecycle` projection collapses the per-stage
// `action_stages` history into the host display shape:
// `{ in_flight: [...], recent_terminal: [...] }`. Each entry carries a
// `correlation_id` plus the latest stage (flattened verbatim from the
// Rust `LifecycleStage` enum — `Failed`'s `reason` lifts to a sibling
// of `stage`). Terminal entries drop on a 3-second TTL inside the
// kernel; the shell does not track them.

/// One stage in the V5 display projection. Mirrors the Rust
/// `LifecycleStage` enum; an unrecognized discriminant collapses to
/// `.unknown(raw:)` so a future kernel stage added without a Swift
/// counterpart does not crash the bridge (D1 — graceful schema growth).
enum ActionLifecycleStage: Equatable {
    case requested
    case awaitingCapability
    case publishing
    case accepted
    /// `reason` is the human-readable failure message the host renders
    /// verbatim. Same field-level shape as `ActionStage.failed`.
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

/// One row in either `inFlight` or `recentTerminal`. The Rust side
/// flattens `stage` and `correlation_id` (and `reason` on `failed`)
/// onto the same object, so the decoder reads them via an explicit
/// `init(from:)` that switches on the `stage` discriminant.
struct ActionLifecycleEntry: Decodable, Equatable, Identifiable {
    let correlationId: String
    let stage: ActionLifecycleStage

    var id: String { correlationId }

    enum CodingKeys: String, CodingKey {
        case correlationId
        case stage
        case reason
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        correlationId = try container.decode(String.self, forKey: .correlationId)
        let raw = try container.decode(String.self, forKey: .stage)
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

    /// Memberwise initializer. The custom `init(from:)` above suppresses
    /// Swift's synthesized memberwise init, so the V6 Stage 4 typed-sidecar glue
    /// (`TypedProjectionGlue.actionLifecycle`) needs this explicit one to build a
    /// row from the `flatc --swift` reader struct (mirroring the
    /// `PublishOutboxRelay` precedent in PR #1053).
    init(correlationId: String, stage: ActionLifecycleStage) {
        self.correlationId = correlationId
        self.stage = stage
    }
}

/// V5 thin-shell display projection. The kernel handles all lifecycle
/// bookkeeping (latest-stage-wins collapse, TTL eviction on terminals).
/// The shell decodes this struct verbatim and renders directly — no
/// pendingActions set, no manual ackActionStage, no PR-G2 race cache.
struct ActionLifecycleSnapshot: Decodable, Equatable {
    /// Correlation_ids whose latest stage is non-terminal
    /// (`requested` / `awaitingCapability` / `publishing`). Render a
    /// spinner per entry. Stable order: first-record first.
    let inFlight: [ActionLifecycleEntry]
    /// Correlation_ids that settled (`accepted` / `failed`) within the
    /// last 3 seconds. Render a success/failure toast per entry; the
    /// kernel drops the entry on its next emit past the TTL. Stable
    /// order: first-record first.
    let recentTerminal: [ActionLifecycleEntry]
}

struct PublishOutboxItem: Decodable, Identifiable, Equatable {
    let handle: String
    let eventId: String
    let kind: UInt32
    let title: String
    let preview: String
    // ADR-0032 / V-115: `createdAtDisplay` removed. Raw Unix-seconds timestamp;
    // shell formats with its own locale/TZ via `UInt64.relativeTimeFromUnixSeconds`.
    let createdAt: UInt64
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
    // ADR-0032 / V-115: `targetSummary` removed. Shell composes
    // "N relays · <time>" from `targetRelays` + `createdAt.relativeTimeFromUnixSeconds`.
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
    /// Pre-formatted English reason the relay was targeted — empty string on
    /// old kernels. Shell renders verbatim with no branching.
    /// `skip_serializing_if = "String::is_empty"` on the Rust side means the
    /// key is absent when empty; `decodeIfPresent` handles that transparently.
    let relayReason: String

    var id: String { relayUrl }

    private enum CodingKeys: String, CodingKey {
        case relayUrl, status, statusLabel, attempt, attemptLabel, message, relayReason
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        relayUrl = try c.decode(String.self, forKey: .relayUrl)
        status = try c.decode(String.self, forKey: .status)
        statusLabel = try c.decode(String.self, forKey: .statusLabel)
        attempt = try c.decode(UInt32.self, forKey: .attempt)
        attemptLabel = try c.decode(String.self, forKey: .attemptLabel)
        message = try c.decode(String.self, forKey: .message)
        relayReason = try c.decodeIfPresent(String.self, forKey: .relayReason) ?? ""
    }

    /// Memberwise initializer. The custom `init(from:)` above suppresses Swift's
    /// synthesized memberwise init, so the V6 Stage 4 typed-sidecar glue
    /// (`TypedProjectionGlue.publishOutbox`) needs this explicit one to build a
    /// row from the `flatc --swift` reader struct.
    init(
        relayUrl: String,
        status: String,
        statusLabel: String,
        attempt: UInt32,
        attemptLabel: String,
        message: String,
        relayReason: String
    ) {
        self.relayUrl = relayUrl
        self.status = status
        self.statusLabel = statusLabel
        self.attempt = attempt
        self.attemptLabel = attemptLabel
        self.message = message
        self.relayReason = relayReason
    }
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

// `AppRelay` and `RelayRoleOption` moved to
// `Generated/KernelTypes.generated.swift` (V6 Stage 1, plan §6b). Rust
// source: `nmp-core/src/kernel/identity_state.rs::AppRelay` /
// `nmp-core/src/actor/relay_roles.rs::RelayRoleOption`. The previous
// `AppRelay` carried a custom memberwise `init(url:role:roleLabel:roleTint:)`
// with defaulted last two args; no caller in the iOS shell constructed
// `AppRelay` directly (only decoded from snapshots and read fields),
// so removing the init is safe — the generated type's synthesised
// memberwise init is unused.

/// NIP-47 wallet connection status, projected from the kernel snapshot.
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// maps Rust snake_case (`balance_sats`, `wallet_npub_short`, …) onto these
/// camelCase properties automatically.
///
/// ADR-0032: `balanceSatsDisplay` and `walletNpubShort` are no longer
/// emitted by the kernel. The presentation layer formats the satoshi
/// balance and abbreviates the wallet npub locally. `isReady` /
/// `isConnected` remain pre-computed because they encode protocol
/// semantics, not display formatting.
struct WalletStatusData: Decodable, Equatable {
    /// `"connecting"` | `"ready"` | `"error"` | `"disconnected"`
    let status: String
    let relayUrl: String
    /// Wallet service pubkey, hex (64 chars). Presentation layer formats
    /// for display (ADR-0032 — bech32 / abbreviation are shell concerns).
    let walletPubkeyHex: String
    let walletNpub: String
    let balanceMsats: UInt64?
    /// Satoshi balance (= `balance_msats / 1000`). `nil` until the wallet
    /// responds to `get_balance`. Presentation layer formats for display.
    let balanceSats: Int?
    /// `status == "ready"` pre-computed in Rust.
    let isReady: Bool
    /// `status == "connecting" || status == "ready"` pre-computed in Rust.
    let isConnected: Bool
}

/// Profile summary card. Raw kind:0 metadata fields — `displayName` and
/// `pictureUrl` are `nil` until a kind:0 has arrived; the presentation
/// layer chooses its own fallback (typically the abbreviated hex pubkey).
/// ADR-0032.
struct ProfileCard: Decodable, Equatable {
    let pubkey: String
    // ADR-0032 / V-115: `npub` (bech32) removed from wire. Shells encode
    // bech32 via `nmp_app_encode_profile(app, pubkey)` or equivalent.
    /// Display name from kind:0 (`display_name` / `displayName` / `name`,
    /// first non-empty wins). `nil` when no kind:0 has arrived yet —
    /// presentation layer renders its own fallback.
    let displayName: String?
    /// Picture URL from kind:0. `nil` when no kind:0 has arrived yet or
    /// the metadata carries no `picture` field — presentation layer
    /// chooses a placeholder strategy.
    let pictureUrl: String?
    let nip05: String
    let about: String
    /// True when a kind:0 metadata event has been received for this
    /// pubkey. False means the card is a placeholder pending relay
    /// response.
    let hasProfile: Bool
    /// NIP-57 lightning address (`lud16`) / LNURL (`lud06`) pre-extracted
    /// from kind:0. `nil` when the user has no lightning address or their
    /// kind:0 hasn't arrived. The zap button is shown only when this is
    /// non-nil — Rust decides zapability, the shell renders (thin-shell
    /// rule).
    let lnurl: String?
}

extension ProfileCard {
    /// Display label for this profile — kind:0 display name when present,
    /// abbreviated hex pubkey otherwise. ADR-0032 fallback owned by the
    /// presentation layer.
    var displayLabel: String { displayName ?? pubkey.shortHex }
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

// V-112 (ADR-0042): `AuthorProfileSnapshot` Decodable deleted — the
// `author_view` projection (and its `authorView` field on the generated
// `SnapshotProjections`) was removed with the kernel author/thread view
// stack. Author rendering reads the per-app FlatFeed
// (`nmp_app_chirp_open_author_feed`); `ProfileAction` /
// `ProfileDispatchSpec` above stay (used by `ProfileView`).

// `TimelineItem` moved to `Generated/KernelTypes.generated.swift` (V6
// Stage 3 partial, plan §6d — F-05). Rust source:
// `nmp-core/src/kernel/types.rs::TimelineItem`. Field docs live alongside
// the Rust definitions.
//
// The generated struct tightens three field-level shapes the hand-written
// version had loosened for "older kernel snapshot" tolerance. The Rust
// kernel always emits all of them — the `decodeIfPresent ?? default`
// fallbacks were dead code, and the schema source of truth now sits on
// the Rust side where it belongs:
//
// 1. `authorPictureUrl` was `String?`; is now `String` (Rust D1 contract:
//    the field is always non-empty — either the kind:0 picture URL or an
//    `identicon:<prefix>` placeholder URI).
// 2. `isRepost`, `navTargetId`, `repostInnerContent` were
//    `decodeIfPresent ?? false / id / ""`; the generated decoder hard-fails
//    if any is absent. Rust `kernel/types.rs::TimelineItem` defines them
//    as non-Option and `kernel/update.rs::timeline_items` populates them
//    on every tick — the fallback was dead.
// 3. `authorAvatarSource` is added as a non-optional `String`. The Rust
//    field is `pub(super) author_avatar_source: String` (kind:0 ↔
//    placeholder discriminator); the hand-written struct never decoded
//    it, so consumers had no way to read the avatar provenance. Adding it
//    is purely additive.
//
// The synthetic-construction call site `ModularBlockView.syntheticItem`
// is updated to provide the new mandatory fields directly.

// `KernelMetrics` and `RelayStatus` moved to
// `Generated/KernelTypes.generated.swift` (V6 Stage 1, plan §6b). Rust
// source: `nmp-core/src/kernel/types.rs::Metrics` /
// `nmp-core/src/kernel/types.rs::RelayStatus`. Field docs live alongside
// the Rust definitions.
//
// The generated `KernelMetrics` adds transport/drop counters the hand-written
// shape was missing — `dispatchDropsTotal`, `claimDropsTotal`, and
// `updateFrameDegradationsTotal` — all non-optional `UInt64`. The Rust kernel
// always emits them
// (`update.rs::metrics_snapshot`), so the now-stricter Swift decode is
// safe against any live snapshot.
//
// The generated `RelayStatus` adds three fields the hand-written shape
// was missing — `errorCategory: String?`, `denied: Bool`, and
// `lastCloseReason: String?` — all currently-emitted by
// `kernel::status::relay_status()`. The `nip77Negentropy` field tightens
// from `String?` to `String` (Rust emits it unconditionally as
// `"unknown" | "probing" | "supported" | "unsupported"`), and
// `bytesRx` / `bytesTx` / `eventsRx` are tightened from optional to
// non-optional to match the Rust definitions.

extension Duration {
    var microseconds: Int {
        let parts = components
        return Int(parts.seconds) * 1_000_000 + Int(parts.attoseconds / 1_000_000_000_000)
    }
}
