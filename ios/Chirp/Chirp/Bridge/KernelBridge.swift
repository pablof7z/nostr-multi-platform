import Darwin
import Foundation
import os.log

let kbLog = Logger(subsystem: "io.f7z.chirp", category: "KernelBridge")

/// Desired subscription shape for a kind:0 profile claim — the 5th
/// `liveness` argument to `nmp_app_claim_profile`. Mirrors the kernel's
/// CacheOk / Live intents (the kernel resolves mixed claims Tailing-wins).
enum ProfileLiveness: Int32 {
    /// Serve from cache; a OneShot kind:0 fetch fills a miss; no live
    /// subscription. Use for feed avatars and inline list contexts.
    case cacheOk = 0
    /// Register a Tailing kind:0 interest so reactive profile-edit updates
    /// flow in. Use for the profile screen.
    case live = 1
}

/// ADR-0063 Lane D (#1671) — the origin-blind reference namespace for the
/// unified `nmp_app_resolve_ref` / `nmp_app_release_ref` C-ABI. Raw values
/// mirror the kernel's integer encoding (`crates/nmp-ffi/src/resolve_ref.rs`):
/// `0` = profile (kind:0), `1` = event.
enum RefNamespace: Int32 {
    case profile = 0
    case event = 1
}

/// ADR-0063 Lane D (#1671) — the requested projection shape for a `resolve_ref`
/// claim. Codes are GLOBALLY UNIQUE across namespaces (the kernel fails closed
/// on a namespace/shape mismatch). Each shape is valid with exactly one
/// namespace:
///   * `.profileRef`  (0) — `{pubkey, display_name, picture_url}` feed-avatar
///     subset; namespace `.profile`. Use for feed cards / search / notifications.
///   * `.profileCard` (1) — full `ProfileCard`; namespace `.profile`. Use for
///     the open profile screen.
///   * `.eventEmbed`  (2) — render-an-embed-card subset; namespace `.event`.
///   * `.eventRaw`    (3) — full raw event; namespace `.event`.
enum RefShape: Int32 {
    case profileRef = 0
    case profileCard = 1
    case eventEmbed = 2
    case eventRaw = 3
}

/// ADR-0063 Lane D (#1671) — liveness intent for a `resolve_ref` claim. `0`
/// (CacheOk) serves from the store with a OneShot fill and no live sub (feed
/// rows / background); non-zero (Live) keeps a tailing sub open while the
/// consumer holds the key (open screen). Reuses the same semantics as the old
/// `ProfileLiveness`, which `resolve_ref` supersedes.
enum RefLiveness: Int32 {
    case cacheOk = 0
    case live = 1
}

/// Mirror of `KERNEL_SCHEMA_VERSION` (Rust: `crates/nmp-core/src/update_envelope.rs`).
/// Must be bumped in lock-step when the Rust constant changes. A mismatch causes
/// `KernelBridge.decode()` to reject the snapshot rather than silently misparse
/// renamed or retyped fields (see `update.rs` contract comment).
private let KERNEL_SCHEMA_VERSION: UInt32 = 1

/// Thin C-FFI wrapper around the `nmp_core` static library.
final class KernelHandle {
    let raw: UnsafeMutableRawPointer
    /// Retained handle for the update sink whose opaque pointer is registered
    /// with Rust via `nmp_app_set_update_callback`. We `passRetained` the sink
    /// into Rust (Rust owns the +1) and hold the `Unmanaged` token here so the
    /// retain can be released *exactly once* — on re-`listen()` (replace) or in
    /// `deinit` (clear). This removes the fragile dependency on `updateSink`
    /// staying non-nil for the registered pointer to remain valid: even if the
    /// strong property were cleared, the Rust-side retain keeps the object
    /// alive until `nmp_app_set_update_callback(raw, nil, nil)` quiesces.
    private var retainedUpdateSink: Unmanaged<KernelUpdateSink>?
    /// Strong reference to the registered capabilities object. Held so the
    /// context pointer passed to `nmpCapabilityCallback` stays valid until
    /// `deinit` unregisters the callback.
    private var retainedCapabilities: ChirpCapabilities?
    /// T146 — opaque handle returned by `nmp_app_chirp_register`. The
    /// modular-timeline bridge extension manages its lifetime; see
    /// `Bridge/ModularTimelineBridge.swift`.
    var chirpHandle: UnsafeMutableRawPointer?
    /// Opaque handle returned by `nmp_marmot_register_active`. The
    /// Marmot bridge extension manages its lifetime; see
    /// `Bridge/MarmotBridge.swift`. Registered lazily once a secret key is
    /// known (nsec sign-in); nil until then (and for bunker sign-in).
    var marmotHandle: UnsafeMutableRawPointer?
    /// ADR-0055 R3-S3: NMP-owned rev-aware projection cache. Lives here (one
    /// instance per kernel) so the cache lifetime exactly matches the kernel
    /// lifetime, and `resetAndRestart()` can call `projectionCache.reset()`.
    let projectionCache = ProjectionMergeCache()
    /// ADR-0063 Lane E (#1671): NMP-owned per-key row cache for the keyed
    /// reference projections (`refs.profile` / `refs.event`). One instance per
    /// kernel (lifetime == kernel lifetime, reset on `reset()`), fed the
    /// row-delta batches in `KernelModel.apply` (on `@MainActor`, so its
    /// per-key `rowChanged` Combine publisher drives SwiftUI). This is the
    /// SOURCE of truth for resolved profiles/events the shell renders via the
    /// `resolve_ref` claim path — there is NO app-side profile cache (D4).
    let keyedRefCache = KeyedRefCache()

    init() {
        raw = nmp_app_new()
        Self.configureStoragePath(for: raw)
        // Stage 4 of NIP-46 wiring: initialise the bunker broker before any
        // `signInBunker(...)` dispatch can reach the actor. The broker
        // registers a hook with `nmp-core` that drives the NIP-46 connect /
        // get_public_key handshake on a worker thread, then translates the
        // broker's signer-ready event into
        // `AddSigner(source: RemoteHandle, make_active:)`.
        let brokerResult = nmp_signer_broker_init(raw)
        if brokerResult != 0 {
            kbLog.fault("nmp_signer_broker_init returned \(brokerResult) — bunker broker NOT active; init logic error")
            assertionFailure("nmp_signer_broker_init failed with code \(brokerResult)")
        }
        // ADR-0053 — declare Chirp's static Tier-2 built-in projection
        // consumption set so the kernel narrows snapshot output to what this
        // shell decodes (the single source of truth is
        // `CHIRP_CONSUMED_BUILTIN_PROJECTIONS` in nmp-app-chirp). Must run
        // before `nmp_app_start`; the kernel stops serializing built-ins this
        // shell never reads. Tier-1 host projections (registered below /
        // per-view feeds) self-gate by registration and are unaffected.
        nmp_app_chirp_declare_consumed_projections(raw)
        // ADR-0055 R3-S3: advertise that this host owns a rev-aware
        // cache-merge layer (the `ProjectionMergeCache`). The kernel uses this
        // to enable omission of Unchanged rows and emission of Cleared rows.
        // Must run BEFORE `nmp_app_start`. Return code contract:
        //   0  = ok
        //   1  = AlreadyStarted (logic error — hard fault in debug)
        //   2  = RegistryUnavailable (internal error — hard fault in debug)
        //  -1  = null app (should never happen here)
        let iaResult = nmp_app_declare_incremental_apply(raw)
        if iaResult != 0 {
            kbLog.fault("nmp_app_declare_incremental_apply returned \(iaResult) — incremental apply NOT active; init logic error")
            assertionFailure("nmp_app_declare_incremental_apply failed with code \(iaResult)")
        }
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
            let status = directory.path.withCString { nmp_app_set_storage_path(raw, $0) }
            if status != 0 {
                kbLog.fault("nmp_app_set_storage_path returned \(status) — persistent storage NOT configured; init logic error")
                assertionFailure("nmp_app_set_storage_path failed with code \(status)")
            }
        } catch {
            kbLog.error("failed to create NMP storage directory: \(error.localizedDescription, privacy: .public)")
        }
    }

    deinit {
        // T146 — drop the projection BEFORE `nmp_app_free` per FFI contract.
        unregisterChirpProjectionIfNeeded()
        // Same contract for the Marmot observer registration.
        unregisterMarmotIfNeeded()
        // Unregister the update callback and release the retained sink in
        // lock-step (balances the `passRetained` in `listen`).
        clearUpdateCallback()
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
        // Clear any prior registration first. `set_update_callback` quiesces
        // (Article: UpdateCallbackGate) — after it returns no in-flight
        // callback can still hold the old context pointer — so releasing the
        // previous retain immediately afterwards is safe.
        clearUpdateCallback()
        let sink = KernelUpdateSink(handler: handler, onPanic: onPanic, cache: projectionCache)
        // `passRetained` hands Rust its own +1 on the sink; the matching
        // release happens in `clearUpdateCallback()` (on replace or deinit).
        let retained = Unmanaged.passRetained(sink)
        retainedUpdateSink = retained
        nmp_app_set_update_callback(
            raw,
            retained.toOpaque(),
            nmpUpdateCallback)
    }

    /// Unregister the Rust update callback and release the sink retain in
    /// lock-step. Idempotent. Relies on the `nmp_app_set_update_callback`
    /// quiescence guarantee: once the setter returns, the actor has drained any
    /// in-flight callback, so no Rust caller can dereference the (about to be
    /// released) context pointer.
    private func clearUpdateCallback() {
        guard let retained = retainedUpdateSink else { return }
        nmp_app_set_update_callback(raw, nil, nil)
        retained.release()
        retainedUpdateSink = nil
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
        nmp_app_start(raw, visibleLimit, emitHz)
    }

    func configure(visibleLimit: UInt32, emitHz: UInt32) {
        nmp_app_configure(raw, visibleLimit, emitHz)
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
    // the Chirp-owned tag-feed seam, which declares primary kind `[1]`, derives
    // NIP-18 repost wrapper acquisition, and opens the compiled `#t` filter at
    // `.global` scope (D0-correct).

    /// M2 (ADR-0042) — generic feed-subscription open. `filterJSON` is a
    /// verbatim NIP-01 REQ filter.
    /// Declared feeds should pass primary kinds only through their typed seam;
    /// protocol adapters derive repost wrappers. `consumerID` refcounts owners so
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
    ///
    /// `liveness` declares the desired subscription shape (see `ProfileLiveness`):
    /// `.cacheOk` (cache + OneShot fill, no live sub) for feed avatars and inline
    /// list contexts; `.live` (Tailing kind:0 interest, reactive profile-edit
    /// updates) for the profile screen. Defaults to `.cacheOk` so the common
    /// list path never opens an unnecessary live subscription.
    func claimProfile(
        pubkey: String,
        consumerID: String,
        force: Bool = false,
        liveness: ProfileLiveness = .cacheOk
    ) {
        pubkey.withCString { pkPtr in
            consumerID.withCString { cidPtr in
                nmp_app_claim_profile(raw, pkPtr, cidPtr, force ? 1 : 0, liveness.rawValue)
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

    /// ADR-0063 Lane E (#1671) — unified, origin-blind reference resolution.
    /// Supersedes `claimProfile` / `claimEvent`: registers (or upgrades) this
    /// `consumerID`'s interest in `(namespace, key)` at the requested `shape`
    /// and `liveness`. The kernel surfaces the resolved entity in the matching
    /// keyed projection (`refs.profile` / `refs.event`) keyed by `key`, which
    /// the `keyedRefCache` consumes on the next frame. Fire-and-forget.
    func resolveRef(
        namespace: RefNamespace,
        key: String,
        consumerID: String,
        shape: RefShape,
        liveness: RefLiveness
    ) {
        key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                nmp_app_resolve_ref(
                    raw, namespace.rawValue, keyPtr, cidPtr,
                    shape.rawValue, liveness.rawValue)
            }
        }
    }

    /// ADR-0063 Lane E (#1671) — release a reference registered via
    /// `resolveRef`. Pass the SAME `namespace` / `key` / `consumerID`.
    func releaseRef(namespace: RefNamespace, key: String, consumerID: String) {
        key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_ref(raw, namespace.rawValue, keyPtr, cidPtr)
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
            defer { nmp_free_string(ptr) }
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
                guard let ptr = nmp_app_nostrconnect_uri(raw, cbPtr) else {
                    return nil
                }
                defer { nmp_free_string(ptr) }
                return String(cString: ptr)
            }
        }
        guard let ptr = nmp_app_nostrconnect_uri(raw, nil) else {
            return nil
        }
        defer { nmp_free_string(ptr) }
        return String(cString: ptr)
    }

    /// Dispatch a `nmp_app_chirp_create_new_account` call.
    ///
    /// Uses the Chirp-owned wrapper (not the generic `nmp_app_create_new_account`)
    /// so the fresh account auto-follows Chirp's product seed set, which lives in
    /// Rust (`nmp_chirp_config::chirp_default_follows`) — the seed pubkeys never
    /// transit this shell (#1493).
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
                _ = nmp_app_chirp_create_new_account(raw, profilePtr, relaysPtr, mls, 1)
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
                defer { nmp_free_string(ptr) }
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
            defer { nmp_free_string(ptr) }
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
    /// §2 #4: writes flow through registered ActionModules, the shell binds
    /// blindly). `bodyJson` is the verbatim string the executor validates,
    /// passed straight to `nmp_app_dispatch_action` without re-serialisation.
    @discardableResult
    func dispatchRawAction(namespace: String, bodyJson: String) -> DispatchResult {
        let envelope: String? = bodyJson.withCString { jsonPtr in
            namespace.withCString { nsPtr in
                guard let ptr = nmp_app_dispatch_action(raw, nsPtr, jsonPtr) else {
                    return nil
                }
                defer { nmp_free_string(ptr) }
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

    /// Seed the Chirp reference relay set. The default relay list lives in Rust
    /// (`nmp-chirp-config`, surfaced via `nmp_app_chirp_seed_default_relays`),
    /// not in Swift (D7 / thin-shell) — the shell no longer hardcodes URLs.
    /// Returns `false` only on a null app handle.
    @discardableResult
    func seedDefaultRelays() -> Bool {
        nmp_app_chirp_seed_default_relays(raw)
    }

    /// Seed relays from a `[["url","role"],…]` JSON array (the `NMP_TEST_RELAYS`
    /// override shape). Parsing/validation live in Rust
    /// (`nmp_app_chirp_seed_relays_from_json`); returns `false` when the JSON is
    /// malformed or empty so the caller can fall back to `seedDefaultRelays()`.
    func seedRelays(fromJSON json: String) -> Bool {
        json.withCString { nmp_app_chirp_seed_relays_from_json(raw, $0) }
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
        nmp_app_chirp_open_home_feed(raw)
    }

    func closeTimeline() {
        nmp_app_chirp_close_home_feed(raw)
    }

    // ── NIP-47 Wallet Connect ─────────────────────────────────────────────
    //
    // #1607: the bespoke nmp_app_wallet_* FFI symbols were deleted (D11 —
    // one action door). All three operations now route through
    // nmp_app_dispatch_action. The bolt11 double-tap guard lives inside
    // WalletPayInvoiceModule (nmp-nip47); a duplicate tap returns a
    // Conflict rejection which is surfaced as a DispatchResult.failure below
    // rather than a silent no-op. The caller (WalletViewModel) may check
    // the DispatchResult and choose to present user-visible feedback.

    /// Connect a NIP-47 wallet. Errors (invalid URI scheme) arrive as
    /// `DispatchResult.failure`; the kernel also emits a `ShowToast` actor
    /// command that surfaces through `last_error_toast` in the snapshot.
    @discardableResult
    func walletConnect(uri: String) -> DispatchResult {
        dispatchAction(namespace: "nmp.wallet.connect",
                       body: ["Connect": ["uri": uri]])
    }

    /// Disconnect the current NIP-47 wallet (fire-and-forget).
    @discardableResult
    func walletDisconnect() -> DispatchResult {
        dispatchRawAction(namespace: "nmp.wallet.disconnect",
                          bodyJson: "\"Disconnect\"")
    }

    /// Pay a Lightning invoice. Returns a `DispatchResult` with the
    /// correlation_id so the caller can drive a payment-progress spinner.
    /// A duplicate bolt11 tap within the TTL window returns
    /// `DispatchResult.failure("payment already in progress…")`.
    @discardableResult
    func walletPayInvoice(bolt11: String, amountMsats: UInt64?) -> DispatchResult {
        var body: [String: Any] = ["bolt11": bolt11]
        if let amount = amountMsats {
            body["amount_msats"] = amount
        } else {
            body["amount_msats"] = NSNull()
        }
        return dispatchAction(namespace: "nmp.wallet.pay_invoice",
                              body: ["PayInvoice": body])
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

    fileprivate static func decodeFlatBuffer(
        bytes: UnsafeRawPointer,
        count: Int,
        cache: ProjectionMergeCache
    ) -> KernelDecodedUpdateFrame? {
        let start = ContinuousClock.now
        let data = Data(bytes: bytes, count: count)
        do {
            let frame = try KernelUpdateFrameDecoder.decode(data)
            guard case let .snapshot(frameSchemaVersion, sessionId, snapshotEpoch, rawEnvelopes, flatFeeds, typedEnvelope) = frame else {
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
            // ADR-0055 R3-S3: Run the cache-merge BEFORE the TypedXDecoder
            // family. The merge re-feeds decoders the FULL merged envelope set
            // (retained cached rows for omitted keys, Cleared keys removed),
            // so they keep their exact current behavior. The merge also
            // surfaces the set of keys whose rev advanced in this frame and
            // the sticky needsResync flag.
            //
            // `sessionId` + `snapshotEpoch` were read off the SAME `frame.snapshot`
            // table in `KernelUpdateFrameDecoder.decode`'s single pass and threaded
            // out through the `.snapshot(...)` case — no second parse of the buffer
            // here (the whole point of this ladder is to stop paying O(buffer) per
            // tick). The `rawEnvelopes` already carry rev+state from
            // `extractTypedProjections`.
            let mergeResult = cache.merge(
                envelopes: rawEnvelopes,
                sessionId: sessionId,
                snapshotEpoch: snapshotEpoch
            )
            let envelopes = mergeResult.mergedEnvelopes
            let changedKeys = mergeResult.changedKeys
            let needsResync = mergeResult.needsResync
            // ADR-0063 Lane E (#1671): the keyed reference projections
            // (`refs.profile` / `refs.event`) are NOT routed through the
            // ProjectionMergeCache (which is keyed per WHOLE projection, not
            // per row). They carry an `nmp.refs.RefRowDeltaBatch` (NRRD) payload
            // that the per-key `KeyedRefCache` merges row-by-row. We carry the
            // RAW pre-merge envelopes through to `KernelModel.apply` so the
            // merge runs on `@MainActor` (its per-key `rowChanged` publisher
            // drives SwiftUI). Filter off `rawEnvelopes` — they hold the
            // verbatim wire payload, untouched by the projection-cache pass.
            let refsRowEnvelopes = rawEnvelopes.filter {
                KeyedRefCache.namespace(forProjectionKey: $0.key) != nil
            }
            if needsResync {
                kbLog.error("ProjectionMergeCache needsResync=true — one or more projection decode-before-commit failures; will be repaired on next genuine rev bump")
            }
            // ADR-0038 typed path: prefer the typed home-feed decode when the
            // NOFS sidecar is present and fully decodable (NFCT bytes filled).
            // Returns nil when absent or malformed → generic path stays active
            // (ADR-0037 Commitment 4 graceful fallback).
            // NOTE: flat feeds are extracted BEFORE the cache merge re-filters
            // the envelope set, so dynamic per-view feeds (author/thread) still
            // route correctly. They are Tier-1 always-Changed so the cache
            // pass-through is a no-op for them.
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
            // #626: NIP-29 group-create defaults (NGDF). The crate-owned
            // suggested public-group relay URL. Output-only projection; the
            // producer registers it once at app init, so the sidecar is present
            // on every tick. Nil only on an older kernel build → the generic
            // `projections["nmp.nip29.group_defaults"]` JSON path applies.
            let typedGroupDefaults = TypedGroupDefaultsDecoder.decode(from: envelopes)
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
            // Issue #1283 Phase 1: the kernel-resolved embed map (`NEMB`). Returns
            // nil when the sidecar is absent/malformed → the generic
            // `projections.claimedEventEmbeds` JSON path stays active (ADR-0037
            // Commitment 4). This is what feeds `EmbedHost` after the in-Swift
            // resolver was deleted.
            let typedClaimedEventEmbeds = TypedClaimedEventEmbedsDecoder.decode(from: envelopes)
            // NIP-46 cluster (`bunker_handshake` / `nip46_onboarding`). Each
            // returns nil when its sidecar is absent/malformed → the generic
            // `projections.<field>` JSON path stays active (ADR-0037 Commitment
            // 4), mirroring `typedAccounts` above. `bunker_handshake`'s typed
            // closure emits NO sidecar while idle (slot is `None`), so nil there
            // is the steady-state — the generic JSON `null` is the fallback.
            let typedBunkerHandshake = TypedBunkerHandshakeDecoder.decode(from: envelopes)
            let typedNip46Onboarding = TypedNip46OnboardingDecoder.decode(from: envelopes)
            // ADR-0048 D6: unified remote-signer health (`signer_state`, KSST —
            // generalises the V-14 `bunker_connection_state` sidecar). Nil while
            // no remote-signer session is active (slot is `None`) — the steady
            // state for local-key accounts. `isReady`/`isAwaitingApproval`/
            // `isReconnecting`/`isUnavailable`/`isFailed` drive status badges
            // for BOTH NIP-46 and NIP-55 backends; no generic JSON fallback
            // needed because iOS has always needed the sidecar (ADR-0037 §4).
            let typedSignerState = TypedSignerStateDecoder.decode(from: envelopes)
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
                    typedGroupDefaults: typedGroupDefaults,
                    typedProfile: typedProfile,
                    typedClaimedProfiles: typedClaimedProfiles,
                    typedResolvedProfiles: typedResolvedProfiles,
                    typedDmInbox: typedDmInbox,
                    typedDmRelayList: typedDmRelayList,
                    typedClaimedEvents: typedClaimedEvents,
                    typedClaimedEventEmbeds: typedClaimedEventEmbeds,
                    typedBunkerHandshake: typedBunkerHandshake,
                    typedNip46Onboarding: typedNip46Onboarding,
                    typedSignerState: typedSignerState,
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
                    decodeMicros: duration.microseconds,
                    changedKeys: changedKeys,
                    needsResync: needsResync,
                    refsRowEnvelopes: refsRowEnvelopes,
                    refsSessionId: sessionId,
                    refsSnapshotEpoch: snapshotEpoch
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
    /// ADR-0055 R3-S3: reference to the per-kernel projection cache so the
    /// callback can run the merge before feeding the decoded frame to the
    /// TypedXDecoder family. Unowned — the cache lifetime is the kernel
    /// lifetime; the sink is always released before the kernel (clearUpdateCallback
    /// runs before nmp_app_free in deinit).
    unowned let cache: ProjectionMergeCache

    init(
        handler: @escaping (KernelUpdateResult) -> Void,
        onPanic: @escaping () -> Void,
        cache: ProjectionMergeCache
    ) {
        self.handler = handler
        self.onPanic = onPanic
        self.cache = cache
    }
}

/// C capability callback — receives `CapabilityRequest` JSON from Rust and
/// returns a malloc-allocated `CapabilityEnvelope` JSON string that Rust frees
/// via `nmp_free_string` / `CString::from_raw`. Uses `strdup` so the
/// allocation is compatible with Rust's `CString::from_raw` on Apple platforms
/// (both use the system malloc allocator).
///
/// There is one C callback for every capability; `ChirpCapabilities.handleJSON`
/// routes the request to the capability owning its `namespace` (keyring). Rust
/// invokes this from the actor thread (never the main thread), so a synchronous
/// capability may block here safely.
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
    guard let frame = KernelHandle.decodeFlatBuffer(
        bytes: UnsafeRawPointer(bytes),
        count: Int(count),
        cache: sink.cache
    ) else {
        return
    }
    switch frame {
    case let .snapshot(result):
        sink.handler(result)
    case .panic:
        sink.onPanic()
    }
}
