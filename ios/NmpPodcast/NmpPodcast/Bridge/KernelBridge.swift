import Foundation
import os.log

let kbLog = Logger(subsystem: "com.podcast.app", category: "KernelBridge")

/// Thin C-FFI wrapper around `libnmp_core.a` + `libnmp_app_podcast.a`. Mirrors
/// `ios/Chirp/Chirp/Bridge/KernelBridge.swift` in shape — `init` allocates the
/// kernel handle, registers the podcast projection, and exposes a small
/// `Sendable`-by-construction Swift API the model layer drives. Every call
/// degrades silently on the Rust side; the Swift bridge stays D6-compliant.
final class KernelHandle {
    let raw: UnsafeMutableRawPointer
    var podcastHandle: UnsafeMutableRawPointer?
    private var updateSink: KernelUpdateSink?

    init() {
        raw = nmp_app_new()
        // Register the podcast projection immediately. The handle is parked
        // here for the lifetime of the bridge; `deinit` tears it down before
        // freeing the kernel.
        podcastHandle = nmp_app_podcast_register(raw)
    }

    deinit {
        if let handle = podcastHandle {
            nmp_app_podcast_unregister(handle)
            podcastHandle = nil
        }
        nmp_app_set_update_callback(raw, nil, nil)
        nmp_app_free(raw)
    }

    func listen(_ handler: @escaping (String) -> Void) {
        let sink = KernelUpdateSink(handler: handler)
        updateSink = sink
        nmp_app_set_update_callback(raw,
                                    Unmanaged.passUnretained(sink).toOpaque(),
                                    nmpUpdateCallback)
    }

    func start(visibleLimit: UInt32 = 0, emitHz: UInt32 = 0) {
        nmp_app_start(raw, 0, visibleLimit, emitHz)
    }

    func stop() {
        nmp_app_stop(raw)
    }

    func lifecycleForeground() {
        nmp_app_lifecycle_foreground(raw)
    }

    func lifecycleBackground() {
        nmp_app_lifecycle_background(raw)
    }

    // ── Relay-edit (NIP-65) ──────────────────────────────────────────────

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

    // ── Podcast projection ───────────────────────────────────────────────

    /// Snapshot the current podcast library as `LibrarySnapshot`.
    /// Returns `nil` only on serialisation failure or before
    /// `nmp_app_podcast_register` succeeded.
    func podcastSnapshot() -> LibrarySnapshot? {
        guard let handle = podcastHandle,
              let ptr = nmp_app_podcast_snapshot(handle) else {
            return nil
        }
        defer { nmp_app_podcast_snapshot_free(ptr) }
        let json = String(cString: ptr)
        guard let data = json.data(using: .utf8) else {
            kbLog.error("podcastSnapshot: bytes not UTF-8")
            return nil
        }
        do {
            return try JSONDecoder().decode(LibrarySnapshot.self, from: data)
        } catch {
            kbLog.error("podcastSnapshot decode failed: \(error.localizedDescription)")
            return nil
        }
    }

    func podcastSubscribe(feedURL: String, title: String?, author: String?) {
        guard let handle = podcastHandle else { return }
        feedURL.withCString { feedPtr in
            withOptionalCString(title) { titlePtr in
                withOptionalCString(author) { authorPtr in
                    nmp_app_podcast_subscribe(handle, feedPtr, titlePtr, authorPtr)
                }
            }
        }
    }

    func podcastUnsubscribe(podcastID: String) {
        guard let handle = podcastHandle else { return }
        podcastID.withCString { nmp_app_podcast_unsubscribe(handle, $0) }
    }
}

// MARK: - Decoded snapshot shape (matches podcast_core::views)

struct PodcastRowPayload: Decodable, Identifiable, Equatable {
    let id: String
    let title: String
    let author: String
    let artwork_url: String?
    let episode_count: UInt32

    var artworkURL: URL? {
        guard let s = artwork_url, !s.isEmpty else { return nil }
        return URL(string: s)
    }

    var episodeCount: UInt32 { episode_count }
}

/// JSON payload from `nmp_app_podcast_snapshot`. Matches the Rust
/// `podcast_core::views::LibraryView` shape. Named `LibrarySnapshot` here to
/// avoid collision with the SwiftUI `LibraryView` (the view that *displays*
/// this data).
struct LibrarySnapshot: Decodable, Equatable {
    let podcasts: [PodcastRowPayload]

    static var empty: LibrarySnapshot { LibrarySnapshot(podcasts: []) }
}

// MARK: - Kernel snapshot envelope (relay projections only)

/// Lightweight decoder for the kernel-wide `{"t":"snapshot","v":{...}}`
/// envelope emitted by `nmp_app_set_update_callback`. NmpPodcast only
/// consumes the relay-edit and relay-status projections — the rest of the
/// snapshot stays opaque so we don't tie this bridge to the full kernel
/// schema (NmpHighlighter / Chirp own the rich decode paths).
struct KernelRelaySnapshot: Decodable, Equatable {
    let relayEditRows: [RelayEditRow]
    let relayStatuses: [RelayKernelStatus]

    private enum CodingKeys: String, CodingKey {
        case relayEditRows
        case relayStatuses
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        relayEditRows = (try? c.decode([RelayEditRow].self, forKey: .relayEditRows)) ?? []
        relayStatuses = (try? c.decode([RelayKernelStatus].self, forKey: .relayStatuses)) ?? []
    }

    /// Try to decode the relay projections from a raw kernel envelope JSON.
    /// Returns nil for non-snapshot frames (e.g. `t=update`) or malformed
    /// payloads — callers should treat that as "no change this tick" (D6).
    static func decode(envelope json: String) -> KernelRelaySnapshot? {
        guard let data = json.data(using: .utf8),
              let outer = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              (outer["t"] as? String) == "snapshot",
              let inner = outer["v"]
        else {
            return nil
        }
        guard let innerData = try? JSONSerialization.data(withJSONObject: inner) else {
            return nil
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try? decoder.decode(KernelRelaySnapshot.self, from: innerData)
    }
}

/// `RelayEditRow` projection from the kernel — the user's NIP-65 list.
struct RelayEditRow: Decodable, Identifiable, Equatable {
    let url: String
    let role: String
    var id: String { url }

    var isRead: Bool { role == "read" || role == "both" }
    var isWrite: Bool { role == "write" || role == "both" }
}

/// Live per-relay status from the kernel snapshot. We keep only the fields
/// NmpPodcast actually surfaces — the kernel emits more (auth state,
/// negentropy, reconnect counts, …) but they aren't useful in this app's
/// settings screen yet. Optional so older / leaner kernels still decode.
struct RelayKernelStatus: Decodable, Equatable, Identifiable {
    let relayUrl: String
    let connection: String
    let lastError: String?
    let bytesRx: UInt64?
    let bytesTx: UInt64?
    let lastConnectedAtMs: UInt64?
    let lastEventAtMs: UInt64?
    let reconnectCount: UInt32?

    var id: String { relayUrl }
    var isConnected: Bool { connection.lowercased() == "connected" }
}

// MARK: - Update callback plumbing

private final class KernelUpdateSink {
    let handler: (String) -> Void
    init(handler: @escaping (String) -> Void) { self.handler = handler }
}

private let nmpUpdateCallback: NmpUpdateCallback = { context, pointer in
    guard let context, let pointer else { return }
    let json = String(cString: pointer)
    let sink = Unmanaged<KernelUpdateSink>.fromOpaque(context).takeUnretainedValue()
    sink.handler(json)
}

// MARK: - Helpers

private func withOptionalCString<Result>(
    _ s: String?,
    _ body: (UnsafePointer<CChar>?) -> Result
) -> Result {
    if let s {
        return s.withCString { body($0) }
    } else {
        return body(nil)
    }
}
