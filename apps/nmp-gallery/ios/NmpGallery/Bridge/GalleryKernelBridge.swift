import Foundation
import os.log

private let kbLog = Logger(subsystem: "org.nmp.gallery", category: "GalleryKernelBridge")

/// Thin Swift wrapper around the gallery's per-app FFI. All relay / network
/// I/O happens inside the kernel that `raw` points at; this class never opens
/// a socket or parses a Nostr event itself.
///
/// Data-flow architecture (CRITICAL):
///   • Profile data arrives via the PUSH callback registered with
///     `nmp_app_set_update_callback`. The JSON the callback receives carries
///     the full snapshot (including `profiles: {…}`) — this is identical to
///     Chirp's update-channel pattern.
///   • `nmp_app_gallery_snapshot` is a status envelope only
///     (`{schema, alive, projections:{}}`); it is NOT a profile source. The
///     gallery does not rely on it for component data.
///
/// Lifetime:
///   1. `init()`         — `nmp_app_new()` then `nmp_app_gallery_register(raw)`.
///   2. `listen(_:)`     — registers the push callback that delivers snapshot JSON.
///   3. `start()`        — turns on the actor.
///   4. `claimProfile/releaseProfile` — refcounted profile interest. The kernel
///      fetches kind:0 from the relay and pushes the result via the callback.
///   5. `dispatchAction` — generic action dispatch (phase 2).
///   6. `deinit`         — clears callback, frees app.
final class GalleryKernelHandle {
    let raw: UnsafeMutableRawPointer
    /// Opaque handle returned by `nmp_app_gallery_register`. The status-envelope
    /// pull (`nmp_app_gallery_snapshot`) uses it; profile data does NOT.
    let galleryHandle: UnsafeMutableRawPointer?
    private var updateSink: GalleryUpdateSink?

    init() {
        raw = nmp_app_new()
        Self.configureStoragePath(for: raw)
        // Phase 1: register the gallery projection on the kernel. The parallel
        // `nmp-app-gallery` crate decides what the projection contributes.
        galleryHandle = nmp_app_gallery_register(raw)
        if galleryHandle == nil {
            kbLog.error("nmp_app_gallery_register returned NULL — gallery projection not registered")
        }
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
            directory.path.withCString { nmp_app_set_storage_path(raw, $0) }
        } catch {
            kbLog.error("failed to create NmpGallery storage dir: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Register the push callback that delivers each snapshot JSON. The closure
    /// is invoked from the kernel actor thread on every emit tick. Callers are
    /// responsible for thread-hopping if they need main-actor isolation.
    func listen(_ handler: @escaping (String) -> Void) {
        let sink = GalleryUpdateSink(handler: handler)
        updateSink = sink
        nmp_app_set_update_callback(
            raw,
            Unmanaged.passUnretained(sink).toOpaque(),
            galleryUpdateCallback)
    }

    /// Configure the kernel and start the actor thread. The arguments mirror
    /// Chirp's defaults: 200 events/second cap, 80 visible items, 4 Hz emit
    /// rate.
    func start() {
        nmp_app_start(raw, 200, 80, 4)
    }

    func stop() {
        nmp_app_stop(raw)
    }

    // ── Profile claim / release ──────────────────────────────────────────

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

    // ── Demo sign-in (phase 2) ───────────────────────────────────────────

    func signInNsec(_ secret: String) {
        secret.withCString { nmp_app_signin_nsec(raw, $0) }
    }

    // ── Generic action dispatch (phase 2) ────────────────────────────────

    /// Dispatch an action through the M6 `ActionModule` family. Returns the
    /// raw JSON envelope returned by Rust (`{"correlation_id":"…"}` on accept,
    /// `{"error":"…"}` on synchronous rejection).
    @discardableResult
    func dispatchAction(namespace: String, body: String) -> String? {
        let ptr: UnsafeMutablePointer<CChar>? = namespace.withCString { nsPtr in
            body.withCString { bodyPtr in
                nmp_app_dispatch_action(raw, nsPtr, bodyPtr)
            }
        }
        guard let ptr else { return nil }
        defer { nmp_app_free_string(ptr) }
        return String(cString: ptr)
    }

    // ── Status-envelope pull (NOT a profile source) ──────────────────────

    /// Pull the gallery's status envelope (`{schema, alive, projections:{}}`).
    /// Returns nil when the projection isn't registered. Use this for
    /// alive-checks / diagnostics only — profile data comes through the push
    /// callback registered via `listen(_:)`.
    func gallerySnapshotJSON() -> String? {
        guard let handle = galleryHandle else { return nil }
        guard let ptr = nmp_app_gallery_snapshot(handle) else { return nil }
        defer { nmp_app_gallery_snapshot_free(ptr) }
        return String(cString: ptr)
    }
}

// MARK: - Update sink

/// Bridge object retained on the Swift side so the C callback's `context`
/// pointer stays valid. The `handler` closure receives the snapshot JSON
/// string (copied out of the C pointer before the closure runs).
private final class GalleryUpdateSink {
    let handler: (String) -> Void

    init(handler: @escaping (String) -> Void) {
        self.handler = handler
    }
}

/// C update callback. Mirrors Chirp's `nmpUpdateCallback` pattern: copies the
/// payload out of the borrowed C string immediately (the pointer is valid
/// only for the duration of this call), then forwards to the Swift sink.
private let galleryUpdateCallback: NmpUpdateCallback = { context, pointer in
    guard let context, let pointer else { return }
    let payload = String(cString: pointer)
    let sink = Unmanaged<GalleryUpdateSink>.fromOpaque(context).takeUnretainedValue()
    sink.handler(payload)
}
