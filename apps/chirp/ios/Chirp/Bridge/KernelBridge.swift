import Darwin
import Foundation
import os.log

let kbLog = Logger(subsystem: "io.f7z.chirp", category: "KernelBridge")

// ADR-0063 Lane E (#1671): the legacy per-kind profile C symbols and the raw
// `resolve_ref` integer ABI are no longer exposed to Swift callers. The shell
// states profile intent as typed values; `KernelBridge+RefOperations` selects
// the corresponding typed FFI adapter.

/// ADR-0063 Lane D (#1671) — requested profile projection shape.
enum RefShape {
    case profileRef
    case profileCard
}

/// ADR-0063 Lane D (#1671) — liveness intent for a profile claim.
enum RefLiveness {
    case cacheOk
    case live
}

/// Mirror of `KERNEL_SCHEMA_VERSION` (Rust: `crates/nmp-core/src/update_envelope.rs`).
/// Must be bumped in lock-step when the Rust constant changes. A mismatch causes
/// `KernelBridge.decode()` to reject the snapshot rather than silently misparse
/// renamed or retyped fields (see `update.rs` contract comment).
let KERNEL_SCHEMA_VERSION: UInt32 = 1

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
    /// Retained capabilities object whose opaque pointer is registered with
    /// Rust via `nmp_app_set_capability_callback`. The setter is quiescent, so
    /// `clearCapabilityCallback()` can release this retain immediately after
    /// unregister/replace returns.
    private var retainedCapabilities: Unmanaged<ChirpCapabilities>?
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
    /// Opaque feed-session handles returned by `nmp_app_open_feed`, keyed by
    /// the projection key the screen reads. Close feeds by these handles only.
    var feedHandlesByKey: [String: String] = [:]

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
        // Register the modular timeline projection through declared observed
        // projections. See `Bridge/ModularTimelineBridge.swift`.
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
        closeAllOpenFeeds()
        // T146 — drop the projection BEFORE `nmp_app_free` per FFI contract.
        unregisterChirpProjectionIfNeeded()
        // Same contract for the Marmot observer registration.
        unregisterMarmotIfNeeded()
        // Unregister the update callback and release the retained sink in
        // lock-step (balances the `passRetained` in `listen`).
        clearUpdateCallback()
        // Unregister the capability callback and release the retained context
        // in lock-step (balances the `passRetained` in
        // `registerCapabilityHandler`).
        clearCapabilityCallback()
        nmp_app_free(raw)
    }

    /// Register the native keyring capability handler. The Rust kernel routes
    /// every keyring `CapabilityRequest` through this seam. Must be called
    /// before `start()` so the handler is in place for any capability requests
    /// the actor issues during startup.
    func registerCapabilityHandler(_ capabilities: ChirpCapabilities) {
        clearCapabilityCallback()
        let retained = Unmanaged.passRetained(capabilities)
        retainedCapabilities = retained
        nmp_app_set_capability_callback(
            raw,
            retained.toOpaque(),
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

    /// Unregister the capability callback and release the retained capability
    /// context. Idempotent. Relies on the
    /// `nmp_app_set_capability_callback` quiescence guarantee: once the setter
    /// returns, no in-flight capability callback can still dereference this
    /// context pointer.
    private func clearCapabilityCallback() {
        guard let retained = retainedCapabilities else { return }
        nmp_app_set_capability_callback(raw, nil, nil)
        retained.release()
        retainedCapabilities = nil
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
}

final class KernelUpdateSink {
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
let nmpCapabilityCallback: NmpCapabilityCallback = { context, requestJSON in
    guard let context, let requestJSON else { return nil }
    let capabilities = Unmanaged<ChirpCapabilities>.fromOpaque(context).takeUnretainedValue()
    let requestStr = String(cString: requestJSON)
    let resultStr = capabilities.handleJSON(requestStr)
    return resultStr.withCString { strdup($0) }
}

let nmpUpdateCallback: NmpUpdateCallback = { context, bytes, count in
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
