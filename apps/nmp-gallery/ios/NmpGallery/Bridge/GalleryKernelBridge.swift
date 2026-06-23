import Foundation
import os.log

private let kbLog = Logger(subsystem: "org.nmp.gallery", category: "GalleryKernelBridge")

/// Thin Swift wrapper around the gallery's per-app FFI. All relay / network
/// I/O happens inside the kernel that `raw` points at; this class never opens
/// a socket or parses a Nostr event itself.
///
/// Data-flow architecture (CRITICAL):
///   • Profile data arrives via the PUSH callback registered with
///     `nmp_app_set_update_callback`. The callback receives a FlatBuffers
///     `UpdateFrame`; the gallery merges its `refs.profile` row-delta batch
///     into the session `GalleryRefProfileStore` and reads the materialised
///     `projections."refs.profile"[pubkey]` card (ADR-0063 #1671).
///   • There is no pull-side snapshot accessor; kernel liveness is observed
///     through `nmp_app_is_alive` and all state arrives via the push callback.
///
/// Lifetime:
///   1. `init()`         — `nmp_app_new()` then `nmp_app_gallery_register(raw)`.
///   2. `listen(_:)`     — registers the push callback that delivers update bytes.
///   3. `start()`        — turns on the actor.
///   4. `addRelay`       — seed bootstrap relay set (cold-start kind:0 / kind:10002
///      routing target when no logged-in user is present).
///   5. `claimProfile`   — component-owned profile interest (routes through
///      `nmp_app_resolve_ref`). The kernel fetches kind:0 and surfaces the
///      resolved ProfileCard under `projections."refs.profile"[pubkey]`.
///   6. `dispatchAction` — generic action dispatch (phase 2).
///   7. `deinit`         — clears callback, frees app.
final class GalleryKernelHandle {
    let raw: UnsafeMutableRawPointer
    private var updateSink: GalleryUpdateSink?
    /// ADR-0063 (#1671) — host-side mirror of the kernel's `refs.profile`
    /// row-delta projection. One per kernel session; threaded into every
    /// snapshot decode so per-key deltas accumulate. Sole app-side profile
    /// store (D4). Freed in `deinit`.
    ///
    /// The C header imports `struct GalleryRefProfileStore *` as
    /// `OpaquePointer?`, so the handle is stored and passed through DIRECTLY —
    /// no `OpaquePointer(...)` wrapping or `UnsafeMutablePointer(...)` casting.
    let refProfileStore: OpaquePointer?

    init() {
        raw = nmp_app_new()
        Self.configureStoragePath(for: raw)
        refProfileStore = nmp_app_gallery_ref_profile_store_new()
        // Phase 1: register the gallery composition on the kernel. The parallel
        // `nmp-app-gallery` crate forwards to `nmp_app_template::register_defaults`;
        // the call is fire-and-forget (D6) — there is no opaque handle to capture
        // because the gallery has no per-app projection mutex.
        nmp_app_gallery_register(raw)
    }

    deinit {
        // Clear the update callback before releasing `updateSink` so no
        // callback fires with a dangling context pointer.
        nmp_app_set_update_callback(raw, nil, nil)
        // NOTE: the gallery FFI doesn't expose an `nmp_app_gallery_unregister`
        // symbol today — the parallel crate is expected to add one for clean
        // teardown. For now the handle is dropped without explicit cleanup;
        // `nmp_app_free` joins the actor thread so any in-flight observer
        // callback is fenced.
        nmp_app_free(raw)
        // ADR-0063 (#1671): release the refs.profile mirror after the kernel is
        // freed (so no in-flight decode can still touch it).
        nmp_app_gallery_ref_profile_store_free(refProfileStore)
    }

    private static func configureStoragePath(for raw: UnsafeMutableRawPointer) {
        guard let base = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first else {
            return
        }
        let directory = base.appendingPathComponent("NmpGallery", isDirectory: true)
        do {
            try FileManager.default.createDirectory(
                at: directory,
                withIntermediateDirectories: true)
            let status = directory.path.withCString { nmp_app_set_storage_path(raw, $0) }
            if status != 0 {
                kbLog.fault("nmp_app_set_storage_path returned \(status) — persistent storage NOT configured; init logic error")
                assertionFailure("nmp_app_set_storage_path failed with code \(status)")
            }
        } catch {
            kbLog.error("failed to create NmpGallery storage dir: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Register the push callback that delivers each FlatBuffers update frame. The closure
    /// is invoked from the kernel actor thread on every emit tick. Callers are
    /// responsible for thread-hopping if they need main-actor isolation.
    func listen(_ handler: @escaping (Data) -> Void) {
        let sink = GalleryUpdateSink(handler: handler)
        updateSink = sink
        nmp_app_set_update_callback(
            raw,
            Unmanaged.passUnretained(sink).toOpaque(),
            galleryUpdateCallback)
    }

    /// Configure the kernel and start the actor thread. The arguments mirror
    /// Chirp's defaults: 80 visible items, 4 Hz emit rate.
    func start() {
        nmp_app_start(raw, 80, 4)
    }

    func stop() {
        nmp_app_stop(raw)
    }

    // ── Profile resolution (ADR-0063 #1671) ──────────────────────────────

    // ADR-0063 (#1671) FFI integer codes for resolve_ref / release_ref.
    /// `namespace` — the profile resolver.
    private static let refNamespaceProfile: Int32 = 0
    /// `shape` — `profile.ref` (`{pubkey, display_name, picture_url}`; avatar/name).
    private static let refShapeProfileRef: Int32 = 0
    /// `liveness` — `CacheOk` (background; no per-row tailing sub).
    private static let refLivenessCacheOk: Int32 = 0

    /// Resolve a visible profile reference for `pubkey` (ADR-0063 #1671 —
    /// supersedes `claimProfile`). The registry widgets call this on mount; the
    /// resolved kind:0 flows back through `refs.profile`. Origin-blind: every
    /// visible author resolves at `profile.ref` / `CacheOk` (the gallery renders
    /// only inline avatars/names, never an open-profile pane).
    func claimProfile(pubkey: String, consumerID: String) {
        pubkey.withCString { pkPtr in
            consumerID.withCString { cidPtr in
                nmp_app_resolve_ref(
                    raw,
                    Self.refNamespaceProfile,
                    pkPtr,
                    cidPtr,
                    Self.refShapeProfileRef,
                    Self.refLivenessCacheOk)
            }
        }
    }

    /// Release a profile reference previously claimed via `claimProfile`. Pass
    /// the SAME `(pubkey, consumerID)` so the kernel reclaims the slot.
    func releaseProfile(pubkey: String, consumerID: String) {
        pubkey.withCString { pkPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_ref(raw, Self.refNamespaceProfile, pkPtr, cidPtr)
            }
        }
    }

    // ── Event claim / release ────────────────────────────────────────────

    /// Claim an embedded event by `nostr:` URI (ADR-0034 / M16). Refcounted
    /// per `consumerID`. The kernel fetches the event via the OneshotApi
    /// (single-writer interest registration — D4) when not yet in the local
    /// store, and surfaces it in the snapshot's
    /// `projections.claimed_events[primary_id]` map.
    ///
    /// Fire-and-forget at the FFI boundary (D6 — silent no-op on null/empty
    /// arguments; the actor owns all error handling).
    /// F-TTL — `force` controls the lazy re-verification gate; it only affects
    /// `naddr` (addressable / replaceable) URIs and is a silent no-op for
    /// immutable `nevent`/`note` URIs. Pass `true` only on explicit user
    /// navigation / pull-to-refresh; default `false` for background claims.
    func claimEvent(uri: String, consumerID: String, force: Bool = false) {
        uri.withCString { uriPtr in
            consumerID.withCString { cidPtr in
                nmp_app_claim_event(raw, uriPtr, cidPtr, force ? 1 : 0)
            }
        }
    }

    /// Release a previously-claimed embedded event. Mirrors `releaseProfile`:
    /// decrements the per-consumer refcount; the kernel drops the row when
    /// the refcount hits zero.
    func releaseEvent(uri: String, consumerID: String) {
        uri.withCString { uriPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_event(raw, uriPtr, cidPtr)
            }
        }
    }

    // ── Relay seeding ────────────────────────────────────────────────────

    /// Add a relay row. The kernel canonicalizes the URL, dials the socket via
    /// `ensure_relay_worker`, and threads the URL into the planner's
    /// `app_relays` set so kind:0 / kind:10002 lookups have a routing target
    /// when there is no logged-in user. `role` accepts `"read"`, `"write"`, or
    /// `"both"`; the gallery seeds indexer/content relays as `"both"` so the
    /// same socket serves both inbox and outbox legs.
    func addRelay(url: String, role: String) {
        url.withCString { uPtr in
            role.withCString { rPtr in
                nmp_app_add_relay(raw, uPtr, rPtr)
            }
        }
    }

    // ── Showcase sign-in (phase 2) ───────────────────────────────────────

    func signInNsec(_ secret: String) {
        secret.withCString { nmp_app_signin_nsec(raw, $0, 1) }
    }

    // ── Generic action dispatch (phase 2 / ADR-0064 Cut-B #1756) ────────────

    /// Dispatch an action through the typed byte doorway.
    ///
    /// Rust encodes `body` into the typed `ActionPayload` FlatBuffers bytes for
    /// `namespace` and dispatches them through `nmp_app_dispatch_action_bytes`.
    /// No JSON crosses the FFI to the kernel. Returns the raw JSON envelope
    /// (`{"correlation_id":"…"}` on accept, `{"error":"…"}` on synchronous
    /// rejection).
    @discardableResult
    func dispatchAction(namespace: String, body: String) -> String? {
        let ptr: UnsafeMutablePointer<CChar>? = namespace.withCString { nsPtr in
            body.withCString { bodyPtr in
                nmp_app_gallery_dispatch_action_bytes(raw, nsPtr, bodyPtr)
            }
        }
        guard let ptr else { return nil }
        defer { nmp_free_string(ptr) }
        return String(cString: ptr)
    }

}

// MARK: - Update sink

/// Bridge object retained on the Swift side so the C callback's `context`
/// pointer stays valid. The `handler` closure receives copied FlatBuffers
/// frame bytes.
private final class GalleryUpdateSink {
    let handler: (Data) -> Void

    init(handler: @escaping (Data) -> Void) {
        self.handler = handler
    }
}

/// C update callback. Copies the borrowed FlatBuffers update frame
/// immediately, then forwards the binary frame to the gallery model.
private let galleryUpdateCallback: NmpUpdateCallback = { context, pointer, len in
    guard let context, let pointer, len > 0 else { return }
    let data = Data(bytes: pointer, count: Int(len))
    let sink = Unmanaged<GalleryUpdateSink>.fromOpaque(context).takeUnretainedValue()
    sink.handler(data)
}

enum GalleryFlatBufferSnapshotDecoder {
    /// Decode one update frame into the gallery snapshot JSON, merging the
    /// frame's `refs.profile` row-delta batch into `store` first (ADR-0063
    /// #1671). `store` MUST be the per-session mirror so per-key deltas
    /// accumulate across frames.
    static func snapshotJSONData(from data: Data, store: OpaquePointer?) -> Data? {
        let ptr: UnsafeMutablePointer<CChar>? = data.withUnsafeBytes { raw -> UnsafeMutablePointer<CChar>? in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress else {
                return nil
            }
            return nmp_app_gallery_snapshot_json_from_update_frame(
                store, base, UInt(data.count))
        }
        guard let ptr else {
            kbLog.error("gallery typed snapshot decode failed")
            return nil
        }
        defer { nmp_free_string(ptr) }
        return String(cString: ptr).data(using: .utf8)
    }
}
