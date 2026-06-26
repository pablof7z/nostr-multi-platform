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
    private var retainedUpdateSink: Unmanaged<GalleryUpdateSink>?
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
        // Clear the update callback before releasing the retained sink so no
        // callback fires with a dangling context pointer.
        clearUpdateCallback()
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
        clearUpdateCallback()
        let sink = GalleryUpdateSink(handler: handler)
        let retained = Unmanaged.passRetained(sink)
        retainedUpdateSink = retained
        nmp_app_set_update_callback(
            raw,
            retained.toOpaque(),
            galleryUpdateCallback)
    }

    private func clearUpdateCallback() {
        guard let retained = retainedUpdateSink else { return }
        nmp_app_set_update_callback(raw, nil, nil)
        retained.release()
        retainedUpdateSink = nil
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

    /// Resolve a visible profile reference for `pubkey` (ADR-0063 #1671 —
    /// supersedes `claimProfile`). The registry widgets call this on mount; the
    /// resolved kind:0 flows back through `refs.profile`. Origin-blind: every
    /// visible author resolves at `profile.ref` / `CacheOk` (the gallery renders
    /// only inline avatars/names, never an open-profile pane).
    func claimProfile(pubkey: String, consumerID: String) {
        pubkey.withCString { pkPtr in
            consumerID.withCString { cidPtr in
                nmp_app_resolve_profile_ref(raw, pkPtr, cidPtr)
            }
        }
    }

    /// Release a profile reference previously claimed via `claimProfile`. Pass
    /// the SAME `(pubkey, consumerID)` so the kernel reclaims the slot.
    func releaseProfile(pubkey: String, consumerID: String) {
        pubkey.withCString { pkPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_profile_ref(raw, pkPtr, cidPtr)
            }
        }
    }

    // ── Event claim / release ────────────────────────────────────────────

    // App-owned URI adapter: decode nostr: via nmp_nip21_decode_uri, then route
    // the raw event key plus decoded relay/author metadata to typed event-ref
    // adapters.

    private struct EventRefFromUri {
        let key: String
        let metadataJson: String
    }

    /// #1726 — Decode a `nostr:` URI and resolve the embedded event via the
    /// typed event-embed ref adapter.
    /// App-local URI adapter over the unified ref-resolution seam.
    func claimEvent(uri: String, consumerID: String, force: Bool = false) {
        guard let eventRef = decodeEventRef(from: uri) else { return }
        eventRef.key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                eventRef.metadataJson.withCString { metadataPtr in
                    if force {
                        nmp_app_resolve_event_embed_live_with_metadata(
                            raw, keyPtr, cidPtr, metadataPtr)
                    } else {
                        nmp_app_resolve_event_embed_with_metadata(
                            raw, keyPtr, cidPtr, metadataPtr)
                    }
                }
            }
        }
    }

    /// #1726 — App-local URI adapter that releases the event via the typed
    /// event-ref adapter.
    func releaseEvent(uri: String, consumerID: String) {
        guard let eventRef = decodeEventRef(from: uri) else { return }
        eventRef.key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_event_ref(raw, keyPtr, cidPtr)
            }
        }
    }

    /// Decode a `nostr:` URI to the canonical event key plus metadata expected by
    /// the kernel:
    ///   - nevent / note  → hex event_id
    ///   - naddr          → canonical coordinate "kind:pubkey:identifier"
    /// Returns nil on decode failure or a non-event URI (D6: silent no-op).
    private func decodeEventRef(from uri: String) -> EventRefFromUri? {
        guard let jsonStr = uri.withCString({ ptr -> String? in
            guard let cResult = nmp_nip21_decode_uri(ptr) else { return nil }
            defer { nmp_free_string(cResult) }
            return String(cString: cResult)
        }) else { return nil }
        guard let jsonData = jsonStr.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: jsonData) as? [String: Any],
              let ok = obj["ok"] as? Bool, ok
        else { return nil }
        let key: String
        switch obj["target"] as? String {
        case "event":
            guard let eventId = obj["event_id"] as? String else { return nil }
            key = eventId
        case "address":
            guard let kind = obj["kind"] as? NSNumber,
                  let pubkey = obj["pubkey"] as? String,
                  let identifier = obj["identifier"] as? String
            else { return nil }
            key = "\(kind.uint32Value):\(pubkey):\(identifier)"
        default:
            return nil
        }
        var metadata: [String: Any] = ["hints": obj["relays"] as? [String] ?? []]
        if let author = obj["author"] as? String {
            metadata["author"] = author
        }
        if let kind = obj["kind"] as? NSNumber {
            metadata["kind"] = kind.uint32Value
        }
        guard let data = try? JSONSerialization.data(withJSONObject: metadata),
              let metadataJson = String(data: data, encoding: .utf8)
        else { return nil }
        return EventRefFromUri(key: key, metadataJson: metadataJson)
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
