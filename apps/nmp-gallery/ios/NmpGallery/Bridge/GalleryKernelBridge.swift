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
///     `UpdateFrame`; the gallery reads `projections.claimed_profiles[pubkey]`
///     for component-owned profile claims, with `author_view` /
    ///     `mention_profiles` as secondary projections for other showcases.
///   • There is no pull-side snapshot accessor; kernel liveness is observed
///     through `nmp_app_is_alive` and all state arrives via the push callback.
///
/// Lifetime:
///   1. `init()`         — `nmp_app_new()` then `nmp_app_gallery_register(raw)`.
///   2. `listen(_:)`     — registers the push callback that delivers update bytes.
///   3. `start()`        — turns on the actor.
///   4. `addRelay`       — seed bootstrap relay set (cold-start kind:0 / kind:10002
///      routing target when no logged-in user is present).
///   5. `claimProfile`   — component-owned profile interest. The kernel fetches
///      kind:0 and surfaces the resolved ProfileCard under
///      `projections.claimed_profiles[pubkey]`.
///   6. `dispatchAction` — generic action dispatch (phase 2).
///   7. `deinit`         — clears callback, frees app.
final class GalleryKernelHandle {
    let raw: UnsafeMutableRawPointer
    private var updateSink: GalleryUpdateSink?

    init() {
        raw = nmp_app_new()
        Self.configureStoragePath(for: raw)
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

    // ── Event claim / release ────────────────────────────────────────────

    /// Claim an embedded event by `nostr:` URI (ADR-0034 / M16). Refcounted
    /// per `consumerID`. The kernel fetches the event via the OneshotApi
    /// (single-writer interest registration — D4) when not yet in the local
    /// store, and surfaces it in the snapshot's
    /// `projections.claimed_events[primary_id]` map.
    ///
    /// Fire-and-forget at the FFI boundary (D6 — silent no-op on null/empty
    /// arguments; the actor owns all error handling).
    func claimEvent(uri: String, consumerID: String) {
        uri.withCString { uriPtr in
            consumerID.withCString { cidPtr in
                nmp_app_claim_event(raw, uriPtr, cidPtr)
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

    /// Open an author view on `pubkey`. The kernel fetches kind:10002 + kind:0
    /// from discovery relays and exposes the resolved ProfileCard under
    /// `projections.author_view.profile` in the push-callback snapshot.
    func openAuthor(pubkey: String) {
        pubkey.withCString { nmp_app_open_author(raw, $0) }
    }

    func closeAuthor(pubkey: String) {
        pubkey.withCString { nmp_app_close_author(raw, $0) }
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
    static func snapshotJSONData(from data: Data) -> Data? {
        guard data.count >= 8,
              data[4] == 0x4e, data[5] == 0x4d, data[6] == 0x50, data[7] == 0x55 else {
            return nil
        }
        do {
            let reader = Reader(data: data)
            let root = Int(try reader.u32(at: 0))
            guard (try reader.u8Field(table: root, index: 0) ?? 0) == 0,
                  let snapshot = try reader.tableField(table: root, index: 1),
                  let payload = try reader.tableField(table: snapshot, index: 1) else {
                return nil
            }
            return try JSONSerialization.data(withJSONObject: try reader.value(table: payload))
        } catch {
            kbLog.error("gallery FlatBuffers snapshot decode failed: \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    private struct Reader {
        let data: Data

        func value(table: Int) throws -> Any {
            switch try u8Field(table: table, index: 0) ?? 0 {
            case 0:
                return NSNull()
            case 1:
                return try boolField(table: table, index: 1) ?? false
            case 2:
                return try i64Field(table: table, index: 2) ?? 0
            case 3:
                return try u64Field(table: table, index: 3) ?? 0
            case 4:
                return try doubleField(table: table, index: 4) ?? 0
            case 5:
                return try stringField(table: table, index: 5) ?? ""
            case 6:
                return try tableVectorField(table: table, index: 6).map { try value(table: $0) }
            case 7:
                var object: [String: Any] = [:]
                for pair in try tableVectorField(table: table, index: 7) {
                    guard let key = try stringField(table: pair, index: 0) else { continue }
                    let nested = try tableField(table: pair, index: 1).map(value(table:)) ?? NSNull()
                    object[key] = nested
                }
                return object
            default:
                return NSNull()
            }
        }

        func tableField(table: Int, index: Int) throws -> Int? {
            guard let field = try field(table: table, index: index) else { return nil }
            return try indirect(at: field)
        }

        func stringField(table: Int, index: Int) throws -> String? {
            guard let field = try field(table: table, index: index) else { return nil }
            return try string(at: indirect(at: field))
        }

        func tableVectorField(table: Int, index: Int) throws -> [Int] {
            guard let field = try field(table: table, index: index) else { return [] }
            let vector = try indirect(at: field)
            let count = Int(try u32(at: vector))
            return try (0..<count).map { item in
                try indirect(at: vector + 4 + item * 4)
            }
        }

        func u8Field(table: Int, index: Int) throws -> UInt8? {
            guard let field = try field(table: table, index: index) else { return nil }
            try range(field, count: 1)
            return data[field]
        }

        func boolField(table: Int, index: Int) throws -> Bool? {
            guard let value = try u8Field(table: table, index: index) else { return nil }
            return value != 0
        }

        func i64Field(table: Int, index: Int) throws -> Int64? {
            guard let field = try field(table: table, index: index) else { return nil }
            return Int64(bitPattern: try u64(at: field))
        }

        func u64Field(table: Int, index: Int) throws -> UInt64? {
            guard let field = try field(table: table, index: index) else { return nil }
            return try u64(at: field)
        }

        func doubleField(table: Int, index: Int) throws -> Double? {
            guard let field = try field(table: table, index: index) else { return nil }
            return Double(bitPattern: try u64(at: field))
        }

        private func field(table: Int, index: Int) throws -> Int? {
            try range(table, count: 4)
            let vtable = table - Int(try i32(at: table))
            try range(vtable, count: 4)
            let length = Int(try u16(at: vtable))
            let entry = vtable + 4 + index * 2
            guard entry + 2 <= vtable + length else { return nil }
            let offset = Int(try u16(at: entry))
            return offset == 0 ? nil : table + offset
        }

        private func indirect(at offset: Int) throws -> Int {
            offset + Int(try u32(at: offset))
        }

        private func string(at offset: Int) throws -> String? {
            let length = Int(try u32(at: offset))
            let start = offset + 4
            try range(start, count: length)
            return String(data: data[start..<start + length], encoding: .utf8)
        }

        func u32(at offset: Int) throws -> UInt32 {
            try range(offset, count: 4)
            return UInt32(data[offset])
                | (UInt32(data[offset + 1]) << 8)
                | (UInt32(data[offset + 2]) << 16)
                | (UInt32(data[offset + 3]) << 24)
        }

        private func u16(at offset: Int) throws -> UInt16 {
            try range(offset, count: 2)
            return UInt16(data[offset]) | (UInt16(data[offset + 1]) << 8)
        }

        private func i32(at offset: Int) throws -> Int32 {
            Int32(bitPattern: try u32(at: offset))
        }

        private func u64(at offset: Int) throws -> UInt64 {
            try range(offset, count: 8)
            var value: UInt64 = 0
            for byte in 0..<8 {
                value |= UInt64(data[offset + byte]) << (byte * 8)
            }
            return value
        }

        private func range(_ offset: Int, count: Int) throws {
            guard offset >= 0, count >= 0, offset + count <= data.count else {
                throw NSError(domain: "GalleryFlatBufferSnapshotDecoder", code: 1)
            }
        }
    }
}
