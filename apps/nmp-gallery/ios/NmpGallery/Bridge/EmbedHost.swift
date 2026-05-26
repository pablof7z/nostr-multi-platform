import Foundation
import Observation
import SwiftUI
import os.log

private let ehLog = Logger(subsystem: "org.nmp.gallery", category: "EmbedHost")

/// Gallery-side mirror of the kernel's `projections.claimed_events` map.
///
/// The renderer (`NostrContentView` / `EmbeddedEvent`) is frontend-driven
/// (ADR-0034 / M16): it walks a content tree, encounters an `EventRef(uri)`,
/// and the `EmbeddedEvent` view fires `sink.claim(uri, consumerId)` via
/// `EventClaimSinkProtocol`. The host (`KernelEventClaimSink`) forwards to
/// `nmp_app_claim_event`. The kernel registers a `OneshotApi` interest,
/// fetches via relays (or cache-hits), and surfaces the resolved event in
/// `snapshot.projections.claimed_events[primary_id]`.
///
/// `EmbedHost` is the gallery's read-side cache of that projection. Each
/// snapshot push calls `update(fromSnapshotJSON:)`; on the next redraw the
/// SwiftUI view tree re-reads `envelopeForURI(_:)` / `envelopeForPrimaryID(_:)`
/// and the registry dispatches to the right renderer.
///
/// Doctrine: D8 — no polling. Updates are push-driven by the snapshot
/// callback; SwiftUI invalidates dependent views via `@Observable`.
@MainActor
@Observable
final class EmbedHost {
    /// Resolved envelopes keyed by `primary_id` (event-id hex for nevent/note,
    /// `"kind:pubkey:d"` coordinate for naddr). Latest-snapshot-wins; rebuilt
    /// from scratch on every non-empty `claimed_events` payload (mirrors the
    /// TUI's `EmbedHostState`).
    private(set) var envelopesByPrimaryID: [String: EmbeddedEventEnvelope] = [:]

    /// Diagnostics — number of resolved envelopes in the current snapshot.
    var count: Int { envelopesByPrimaryID.count }

    /// Rebuild the in-memory envelope map from a freshly pushed kernel
    /// snapshot. The kernel emits `projections.claimed_events[primary_id]
    /// → ClaimedEventDto`; we route each entry through a Swift mirror of
    /// the Rust `resolve_embed_projection` dispatch (the single
    /// kind-decision point that ADR-0034 mandates).
    ///
    /// Non-fatal: malformed entries are silently skipped — the renderer
    /// falls back to a loading placeholder until a well-formed snapshot
    /// lands (D6).
    func update(fromSnapshotJSON snapshot: [String: Any]) {
        guard let projections = snapshot["projections"] as? [String: Any],
              let claimed = projections["claimed_events"] as? [String: Any]
        else {
            // Missing projection key — leave existing state intact (matches
            // the TUI's `snapshot_without_claimed_events_leaves_host_untouched`
            // semantics).
            return
        }

        var next: [String: EmbeddedEventEnvelope] = [:]
        for (primaryID, raw) in claimed {
            guard let dto = raw as? [String: Any] else { continue }
            guard let envelope = envelope(primaryID: primaryID, dto: dto) else {
                continue
            }
            next[primaryID] = envelope
        }
        envelopesByPrimaryID = next
    }

    /// Lookup an envelope by `primary_id`. Used by `EmbeddedEvent` after the
    /// renderer's URI → primary-id resolution (the gallery indexes by both
    /// keys for fast lookup).
    func envelopeForPrimaryID(_ id: String) -> EmbeddedEventEnvelope? {
        envelopesByPrimaryID[id]
    }

    /// Lookup an envelope by the original `nostr:` URI. Tries the URI as a
    /// direct key, then walks the value set looking for a matching `uri`
    /// (rare — only when the snapshot used a different key than the
    /// renderer-side URI parse would).
    func envelopeForURI(_ uri: String) -> EmbeddedEventEnvelope? {
        if let direct = envelopesByPrimaryID[uri] {
            return direct
        }
        // Linear scan only on miss. Map is small (one entry per claimed embed).
        return envelopesByPrimaryID.values.first { $0.uri == uri }
    }

    // MARK: - Resolver — mirror of nmp_content::resolve_embed_projection

    /// Decode one `ClaimedEventDto` from `projections.claimed_events` and
    /// wrap it in an `EmbeddedEventEnvelope`. The branch on `kind` matches
    /// `crates/nmp-content/src/embed_projection/mod.rs::resolve_embed_projection`.
    private func envelope(primaryID: String, dto: [String: Any]) -> EmbeddedEventEnvelope? {
        guard let id = dto["id"] as? String,
              let authorPubkey = dto["author_pubkey"] as? String
        else {
            return nil
        }
        let kind: UInt32
        if let value = dto["kind"] as? UInt32 {
            kind = value
        } else if let value = dto["kind"] as? Int {
            kind = UInt32(value)
        } else if let value = dto["kind"] as? Double {
            kind = UInt32(value)
        } else {
            return nil
        }
        let createdAt: UInt64
        if let value = dto["created_at"] as? UInt64 {
            createdAt = value
        } else if let value = dto["created_at"] as? Int {
            createdAt = UInt64(value)
        } else if let value = dto["created_at"] as? Double {
            createdAt = UInt64(value)
        } else {
            createdAt = 0
        }
        let content = dto["content"] as? String ?? ""
        let tags: [[String]] = decodeTags(dto["tags"])

        let projection = resolve(
            kind: kind,
            id: id,
            authorPubkey: authorPubkey,
            createdAt: createdAt,
            content: content,
            tags: tags
        )

        return EmbeddedEventEnvelope(
            uri: "",
            primaryId: primaryID,
            projection: projection
        )
    }

    /// Mirror of `nmp_content::resolve_embed_projection`. Stays a pure
    /// function so it can be unit-tested without a kernel; the Rust source
    /// is the canonical reference if these diverge.
    private func resolve(
        kind: UInt32,
        id: String,
        authorPubkey: String,
        createdAt: UInt64,
        content: String,
        tags: [[String]]
    ) -> EmbedKindProjection {
        let tagValue: (String) -> String? = { key in
            // tags is [[String]] — `dropFirst().first` returns a String? when
            // applied to ArraySlice<String>. The compiler infers it correctly
            // because ArraySlice<String>.Element == String.
            guard let row = tags.first(where: { $0.first == key }) else {
                return nil
            }
            return row.count > 1 ? row[1] : nil
        }
        switch kind {
        case 0:
            // Profile content is JSON-encoded NIP-01 metadata. Parse the
            // standard fields; non-JSON content yields a bare projection.
            let meta = parseProfileMetadata(content)
            return .profile(ProfileProjection(
                pubkey: authorPubkey,
                displayName: meta["name"] ?? meta["display_name"],
                pictureUrl: meta["picture"],
                about: meta["about"],
                nip05: meta["nip05"],
                lud16: meta["lud16"],
                bannerUrl: meta["banner"]
            ))
        case 1:
            return .shortNote(ShortNoteProjection(
                id: id,
                authorPubkey: authorPubkey,
                createdAt: createdAt,
                content: content,
                mediaUrls: extractTopLevelMedia(content)
            ))
        case 9802:
            return .highlight(HighlightProjection(
                id: id,
                authorPubkey: authorPubkey,
                createdAt: createdAt,
                highlightedText: content,
                sourceEventId: tagValue("e"),
                sourceEventAddr: tagValue("a"),
                sourceUrl: tagValue("r"),
                context: tagValue("context")
            ))
        case 30023:
            return .article(ArticleProjection(
                id: id,
                authorPubkey: authorPubkey,
                createdAt: createdAt,
                title: tagValue("title"),
                summary: tagValue("summary"),
                heroImageUrl: tagValue("image"),
                dTag: tagValue("d") ?? "",
                content: content
            ))
        default:
            return .unknown(UnknownProjection(
                kind: kind,
                authorPubkey: authorPubkey,
                createdAt: createdAt,
                content: content,
                tags: tags,
                altText: tagValue("alt")
            ))
        }
    }
}

// MARK: - Helpers

private func decodeTags(_ raw: Any?) -> [[String]] {
    guard let arr = raw as? [Any] else { return [] }
    return arr.compactMap { row in
        guard let row = row as? [Any] else { return nil }
        return row.compactMap { $0 as? String }
    }
}

/// Parse a kind:0 profile JSON `content` string. Returns the recognised
/// string-valued NIP-01 fields; absent / non-string fields are dropped.
private func parseProfileMetadata(_ content: String) -> [String: String] {
    guard !content.isEmpty,
          let data = content.data(using: .utf8),
          let parsed = try? JSONSerialization.jsonObject(with: data),
          let dict = parsed as? [String: Any]
    else {
        return [:]
    }
    var out: [String: String] = [:]
    for key in ["name", "display_name", "picture", "about", "nip05", "lud16", "banner"] {
        if let value = dict[key] as? String, !value.isEmpty {
            out[key] = value
        }
    }
    return out
}

/// Mirror of `extract_top_level_media` in nmp-content — pulls http(s) URLs
/// with image/video/audio extensions out of the raw content body.
private func extractTopLevelMedia(_ content: String) -> [String] {
    let exts = [".jpg", ".jpeg", ".png", ".gif", ".webp",
                ".mp4", ".mov", ".webm", ".mp3", ".wav"]
    return content.split(whereSeparator: { $0.isWhitespace }).compactMap { token in
        let lower = token.lowercased()
        guard lower.hasPrefix("http://") || lower.hasPrefix("https://") else {
            return nil
        }
        guard exts.contains(where: lower.hasSuffix) else { return nil }
        return String(token)
    }
}

// MARK: - Environment wiring

private struct EmbedHostKey: EnvironmentKey {
    static let defaultValue: EmbedHost? = nil
}

private struct EmbedClaimSinkKey: EnvironmentKey {
    static let defaultValue: EventClaimSinkProtocol? = nil
}

private struct NostrKindRegistryKey: EnvironmentKey {
    @MainActor static let defaultValue: NostrKindRegistry = NostrKindRegistry.makeDefault()
}

extension EnvironmentValues {
    var embedHost: EmbedHost? {
        get { self[EmbedHostKey.self] }
        set { self[EmbedHostKey.self] = newValue }
    }

    var embedClaimSink: EventClaimSinkProtocol? {
        get { self[EmbedClaimSinkKey.self] }
        set { self[EmbedClaimSinkKey.self] = newValue }
    }

    var nostrKindRegistry: NostrKindRegistry {
        get { self[NostrKindRegistryKey.self] }
        set { self[NostrKindRegistryKey.self] = newValue }
    }
}
