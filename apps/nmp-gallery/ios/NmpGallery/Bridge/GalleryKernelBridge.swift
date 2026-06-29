import Foundation
import os.log

private let kbLog = Logger(subsystem: "org.nmp.gallery", category: "GalleryKernelBridge")

/// Thin Swift wrapper around the gallery's NmpApp instance. All relay / network
/// I/O happens inside the kernel; this class never opens a socket or parses a
/// Nostr event itself.
///
/// Data-flow architecture (CRITICAL):
///   • Profile data arrives via the push sink registered with
///     `setUpdateSink`. The sink receives a FlatBuffers `UpdateFrame`; the
///     gallery merges its `refs.profile` row-delta batch into the session
///     `GalleryRefStores` and reads the materialised
///     `projections."refs.profile"[pubkey]` card (ADR-0063 #1671).
///   • Event embed envelopes follow the same path through `refs.event`: Rust
///     merges the event row-delta store, kind-dispatches with `nmp-content`,
///     and materialises `projections."refs.event.envelopes"[primaryId]`.
///   • There is no pull-side snapshot accessor; all state arrives via the
///     push sink.
///
/// M14 migration: this class previously used the nmp-ffi C-ABI symbols
/// (`nmp_app_new`, `nmp_app_start`, etc.). It now uses the UniFFI-generated
/// `NmpApp` Swift class from `nmp_uniffi.swift`. The app-owned gallery C-ABI
/// (`nmp_app_gallery_*` symbols) is kept as-is per M14 scope rules.
///
/// Lifetime:
///   1. `init()`              — `NmpApp()` + `nmp_app_gallery_register_uniffi`.
///   2. `listen(_:)`          — registers the push sink that delivers update bytes.
///   3. `start()`             — turns on the actor.
///   4. `addRelay`            — seed bootstrap relay set.
///   5. `resolveProfileRef`   — component-owned profile interest.
///   6. `deinit`              — clears sink, NmpApp freed by ARC.
final class GalleryKernelHandle {
    private let app: NmpApp
    private var updateSink: GalleryUpdateSink?
    /// ADR-0063 (#1671) — host-side mirrors of the kernel's `refs.profile`
    /// and `refs.event` row-delta projections. One per kernel session. Freed
    /// in `deinit`.
    let refStores: OpaquePointer?

    init() {
        let app = NmpApp()
        Self.configureStoragePath(for: app)
        refStores = nmp_app_gallery_ref_stores_new()
        // Register the gallery composition on the UniFFI NmpApp.
        // `uniffiClonePointer()` bumps the Arc refcount;
        // `nmp_app_gallery_register_uniffi` takes ownership and releases the
        // clone when it returns (Arc::from_raw semantics on the Rust side).
        nmp_app_gallery_register_uniffi(app.uniffiClonePointer())
        self.app = app
    }

    deinit {
        // Clear the sink before releasing so no callback fires during teardown
        // (quiescence contract — mirrors the old nmp_app_set_update_callback
        // guarantee from the C-ABI path).
        clearUpdateSink()
        // NmpApp is freed when ARC releases the last strong reference here.
        nmp_app_gallery_ref_stores_free(refStores)
    }

    /// Configure the persistent LMDB storage path for the kernel.
    ///
    /// `NmpApp.setStoragePath` is not yet in the ns-ctail-uniffi-drain generated
    /// Swift surface (that method lands with C6). This helper bridges through
    /// `nmp_uniffi_set_storage_path` which extracts the inner RuntimeApp pointer
    /// from the UniFFI Arc and delegates to `nmp_app_set_storage_path`. The Arc
    /// clone passed to the helper is consumed by it (ownership transfer).
    private static func configureStoragePath(for app: NmpApp) {
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
            let status = directory.path.withCString { pathPtr in
                nmp_uniffi_set_storage_path(app.uniffiClonePointer(), pathPtr)
            }
            if status != 0 {
                kbLog.fault("nmp_uniffi_set_storage_path returned \(status) — persistent storage NOT configured")
                assertionFailure("nmp_uniffi_set_storage_path failed with code \(status)")
            }
        } catch {
            kbLog.error("failed to create NmpGallery storage dir: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Register the push sink that delivers each FlatBuffers update frame. The
    /// sink is invoked from the kernel actor thread on every emit tick. Callers
    /// are responsible for thread-hopping if they need main-actor isolation.
    func listen(_ handler: @escaping (Data) -> Void) {
        clearUpdateSink()
        let sink = GalleryUpdateSink(handler: handler)
        updateSink = sink
        app.setUpdateSink(sink: sink)
    }

    private func clearUpdateSink() {
        app.setUpdateSink(sink: nil)
        updateSink = nil
    }

    /// Configure the kernel and start the actor thread.
    func start() {
        app.start(visibleLimit: 80, emitHz: 4)
    }

    func stop() {
        app.stop()
    }

    // ── Profile resolution (ADR-0063 #1671) ──────────────────────────────

    /// Resolve a visible profile reference for `pubkey` through the UniFFI
    /// `resolveProfileRef` (ADR-0063 #1671). The resolved kind:0 flows back
    /// through `refs.profile`.
    func resolveProfileRef(pubkey: String, consumerID: String) {
        app.resolveProfileRef(key: pubkey, consumerId: consumerID)
    }

    /// Release a profile reference previously resolved via `resolveProfileRef`.
    func releaseProfileRef(pubkey: String, consumerID: String) {
        app.releaseProfileRef(key: pubkey, consumerId: consumerID)
    }

    // ── Event-ref resolve / release ──────────────────────────────────────

    private struct EventRefDecoded {
        let key: String
        let metadata: ResolveMetadata
    }

    /// Decode a `nostr:` URI to the canonical event key plus typed metadata.
    /// Uses the UniFFI `decodeNostrUri` function (typed output replaces the
    /// C-ABI `nmp_nip21_decode_uri` + JSON parse path). Returns nil on failure
    /// or a non-event URI (D6: silent no-op).
    private func decodeEventRef(from uri: String) -> EventRefDecoded? {
        guard let target = try? decodeNostrUri(input: uri) else { return nil }
        switch target {
        case .event(let eventId, let relays, let author, _):
            return EventRefDecoded(
                key: eventId,
                metadata: ResolveMetadata(hints: relays, eventAuthor: author))
        case .address(let identifier, let pubkey, let kind, let relays):
            let key = "\(kind):\(pubkey):\(identifier)"
            return EventRefDecoded(
                key: key,
                metadata: ResolveMetadata(hints: relays, eventAuthor: nil))
        default:
            return nil
        }
    }

    /// Decode a `nostr:` URI and resolve the embedded event via the typed
    /// event-embed ref adapter.
    func resolveEventRef(uri: String, consumerID: String, force: Bool = false) {
        guard let eventRef = decodeEventRef(from: uri) else { return }
        if force {
            app.resolveEventEmbedLiveWithMetadata(
                key: eventRef.key,
                consumerId: consumerID,
                metadata: eventRef.metadata)
        } else {
            app.resolveEventEmbedWithMetadata(
                key: eventRef.key,
                consumerId: consumerID,
                metadata: eventRef.metadata)
        }
    }

    /// Release an event previously resolved via `resolveEventRef`.
    func releaseEventRef(uri: String, consumerID: String) {
        guard let eventRef = decodeEventRef(from: uri) else { return }
        app.releaseEventRef(key: eventRef.key, consumerId: consumerID)
    }

    // ── Relay seeding ────────────────────────────────────────────────────

    /// Add a relay row. The kernel canonicalizes the URL and dials the socket.
    func addRelay(url: String, role: String) {
        app.addRelay(url: url, role: role)
    }

    // ── Showcase sign-in (phase 2) ───────────────────────────────────────

    func signInNsec(_ secret: String) {
        app.signinNsec(secret: secret, makeActive: true)
    }
}

// MARK: - Update sink

/// UniFFI `UpdateSink` conformer retained on the Swift side so the kernel's
/// update-listener closure can call back into Swift. The `handler` closure
/// receives copied FlatBuffers frame bytes.
///
/// `@unchecked Sendable`: the `handler` closure may capture non-Sendable state
/// (e.g. a weak `GalleryModel` reference via `Task { @MainActor [weak self] in
/// ... }`); thread-safety is enforced by the MainActor hop inside the closure.
private final class GalleryUpdateSink: UpdateSink, @unchecked Sendable {
    let handler: (Data) -> Void

    init(handler: @escaping (Data) -> Void) {
        self.handler = handler
    }

    func onUpdate(frame: Data) {
        handler(frame)
    }
}

enum GalleryFlatBufferSnapshotDecoder {
    /// Decode one update frame into the gallery snapshot JSON, merging the
    /// frame's `refs.profile` / `refs.event` row-delta batches into `stores`
    /// first (ADR-0063 #1671). `stores` MUST be the per-session mirror so
    /// per-key deltas accumulate across frames.
    static func snapshotJSONData(from data: Data, stores: OpaquePointer?) -> Data? {
        let ptr: UnsafeMutablePointer<CChar>? = data.withUnsafeBytes { raw -> UnsafeMutablePointer<CChar>? in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress else {
                return nil
            }
            return nmp_app_gallery_snapshot_json_from_update_frame(
                stores, base, UInt(data.count))
        }
        guard let ptr else {
            kbLog.error("gallery typed snapshot decode failed")
            return nil
        }
        defer { nmp_free_string(ptr) }
        return String(cString: ptr).data(using: .utf8)
    }
}
