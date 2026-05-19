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
