import Foundation
import Observation
import SwiftUI

/// Raw wire shape for one entry in `projections["claimed_events"]` (ADR-0034 /
/// F-CR-06). Hand-declared value type for the generated
/// `SnapshotProjections.claimedEvents` field — Stage-2 codegen emits the
/// `claimedEvents` field (see
/// `crates/nmp-codegen/src/swift_projections_registry.rs`) but references this
/// value type by name; per the registry's maintenance contract the value type
/// itself stays hand-written until the Stage-3 sweep. Mirrors the kernel's
/// `ClaimedEventDto` (`crates/nmp-core/src/kernel/types.rs`). The kernel emits
/// snake_case keys; `KernelHandle.decode` applies `.convertFromSnakeCase`, so
/// the property names below are the post-transform camelCase form. Extra
/// kernel fields (`primaryId`, author profile hints) decode-tolerant: Codable
/// ignores wire keys with no matching property, so the renderer-relevant
/// subset below is all this host consumes.
struct ClaimedEventDto: Decodable, Equatable {
    let id: String
    let authorPubkey: String
    let kind: Int
    let createdAt: Int
    let content: String
    let tags: [[String]]
}

/// Chirp mirror of the gallery EmbedHost. Reads claimed_events from the
/// typed SnapshotProjections pushed by the kernel on every frame (D8 — push-driven).
@MainActor
@Observable
final class EmbedHost {
    private(set) var envelopesByPrimaryID: [String: EmbeddedEventEnvelope] = [:]
    var count: Int { envelopesByPrimaryID.count }

    /// Called on every snapshot tick. Rebuilds the envelope map from
    /// projections.claimedEvents (snake_case decoded by FlatBufferValueDecoder).
    func update(from projections: SnapshotProjections?) {
        guard let claimed = projections?.claimedEvents, !claimed.isEmpty else { return }
        var next: [String: EmbeddedEventEnvelope] = [:]
        for (primaryID, dto) in claimed {
            guard let envelope = envelope(primaryID: primaryID, dto: dto) else { continue }
            next[primaryID] = envelope
        }
        envelopesByPrimaryID = next
    }

    func envelopeForPrimaryID(_ id: String) -> EmbeddedEventEnvelope? {
        envelopesByPrimaryID[id]
    }

    func envelopeForURI(_ uri: String) -> EmbeddedEventEnvelope? {
        if let direct = envelopesByPrimaryID[uri] { return direct }
        return envelopesByPrimaryID.values.first { $0.uri == uri }
    }

    // MARK: - Resolver

    private func envelope(primaryID: String, dto: ClaimedEventDto) -> EmbeddedEventEnvelope? {
        let kind = UInt32(dto.kind)
        let createdAt = UInt64(max(0, dto.createdAt))
        let projection = resolve(
            kind: kind,
            id: dto.id,
            authorPubkey: dto.authorPubkey,
            createdAt: createdAt,
            content: dto.content,
            tags: dto.tags
        )
        return EmbeddedEventEnvelope(
            uri: "",
            primaryId: primaryID,
            projection: projection
        )
    }

    private func resolve(kind: UInt32, id: String, authorPubkey: String,
                         createdAt: UInt64, content: String, tags: [[String]]) -> EmbedKindProjection {
        let tagValue: (String) -> String? = { key in
            guard let row = tags.first(where: { $0.first == key }) else { return nil }
            return row.count > 1 ? row[1] : nil
        }
        switch kind {
        case 0:
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
                id: id, authorPubkey: authorPubkey, createdAt: createdAt,
                content: content, mediaUrls: extractTopLevelMedia(content)
            ))
        case 9802:
            return .highlight(HighlightProjection(
                id: id, authorPubkey: authorPubkey, createdAt: createdAt,
                highlightedText: content,
                sourceEventId: tagValue("e"), sourceEventAddr: tagValue("a"),
                sourceUrl: tagValue("r"), context: tagValue("context")
            ))
        case 30023:
            return .article(ArticleProjection(
                id: id, authorPubkey: authorPubkey, createdAt: createdAt,
                title: tagValue("title"), summary: tagValue("summary"),
                heroImageUrl: tagValue("image"), dTag: tagValue("d") ?? "",
                content: content
            ))
        default:
            return .unknown(UnknownProjection(
                kind: kind, authorPubkey: authorPubkey, createdAt: createdAt,
                content: content, tags: tags, altText: tagValue("alt")
            ))
        }
    }
}

// MARK: - Helpers

private func parseProfileMetadata(_ content: String) -> [String: String] {
    guard !content.isEmpty,
          let data = content.data(using: .utf8),
          let parsed = try? JSONSerialization.jsonObject(with: data),
          let dict = parsed as? [String: Any]
    else { return [:] }
    var out: [String: String] = [:]
    for key in ["name", "display_name", "picture", "about", "nip05", "lud16", "banner"] {
        if let value = dict[key] as? String, !value.isEmpty { out[key] = value }
    }
    return out
}

private func extractTopLevelMedia(_ content: String) -> [String] {
    let exts = [".jpg", ".jpeg", ".png", ".gif", ".webp", ".mp4", ".mov", ".webm", ".mp3", ".wav"]
    return content.split(whereSeparator: { $0.isWhitespace }).compactMap { token in
        let lower = token.lowercased()
        guard lower.hasPrefix("http://") || lower.hasPrefix("https://") else { return nil }
        guard exts.contains(where: lower.hasSuffix) else { return nil }
        return String(token)
    }
}

// MARK: - Environment keys

private struct EmbedHostKey: EnvironmentKey {
    static let defaultValue: EmbedHost? = nil
}
private struct EmbedClaimSinkKey: EnvironmentKey {
    static let defaultValue: EventClaimSinkProtocol? = nil
}
private struct NostrKindRegistryKey: EnvironmentKey {
    static let defaultValue: NostrKindRegistry? = nil
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
    var nostrKindRegistry: NostrKindRegistry? {
        get { self[NostrKindRegistryKey.self] }
        set { self[NostrKindRegistryKey.self] = newValue }
    }
}
