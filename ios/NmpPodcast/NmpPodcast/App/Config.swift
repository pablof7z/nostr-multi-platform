import Foundation
import OSLog

/// T156 — Static config keys for the Podcast Index API. Mirrors the canonical
/// app's `Config.swift` shape. Once the kernel's `KeyValueStoreCapability`
/// lands (M11 step 2), validation moves to Rust-side bootstrap and this file
/// becomes a thin read-only Swift shim. For now it stays minimal: search and
/// discovery features that depend on the API key are not in this iteration's
/// scope (LibraryView only).
enum Config {
    static var podcastIndexAPIKey: String {
        ProcessInfo.processInfo.environment["PODCAST_INDEX_API_KEY"]
            ?? Bundle.main.object(forInfoDictionaryKey: "PODCAST_INDEX_API_KEY") as? String
            ?? ""
    }

    static var podcastIndexAPISecret: String {
        ProcessInfo.processInfo.environment["PODCAST_INDEX_API_SECRET"]
            ?? Bundle.main.object(forInfoDictionaryKey: "PODCAST_INDEX_API_SECRET") as? String
            ?? ""
    }

    static var isPodcastIndexConfigured: Bool {
        !podcastIndexAPIKey.isEmpty && !podcastIndexAPISecret.isEmpty
    }
}
