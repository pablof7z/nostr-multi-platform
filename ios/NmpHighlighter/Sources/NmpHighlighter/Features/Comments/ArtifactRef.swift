import Foundation

/// The "thing being commented on" in NIP-22 terms. Maps any in-app
/// artifact (article, podcast, book, website, highlight) onto the three
/// allowed root scope tags — uppercase `A` for addressable, `E` for an
/// event id, `I` for external content (NIP-73 identifiers).
///
/// Swift owns this mapping; the Rust core takes raw `(rootTagName,
/// rootTagValue, rootKind)` so the comment surface is generic.
enum ArtifactRef: Hashable {
    /// Addressable artifact: NIP-23 article (`30023:<pubkey>:<d>`),
    /// long-form catalogue items, anything reachable via a `kind:pubkey:d`
    /// triple.
    case article(addr: String, kind: UInt16 = 30023)

    /// Any non-replaceable event addressed by id — most commonly a
    /// kind:9802 highlight, or a kind:11 share.
    case event(id: String, kind: UInt16)

    /// NIP-73 external content. `id` is the `i`-tag value
    /// (`url:<href>`, `podcast:item:guid:<guid>`, `isbn:<isbn>`, …);
    /// `kind` is the host kind (e.g. the wrapping kind:11 share) or `0`
    /// when there is none.
    case external(id: String, kind: UInt16)

    var rootTagName: String {
        switch self {
        case .article: return "A"
        case .event: return "E"
        case .external: return "I"
        }
    }

    var rootTagValue: String {
        switch self {
        case .article(let addr, _): return addr
        case .event(let id, _): return id
        case .external(let id, _): return id
        }
    }

    var rootKind: UInt16 {
        switch self {
        case .article(_, let k): return k
        case .event(_, let k): return k
        case .external(_, let k): return k
        }
    }
}

extension ArtifactRef {
    /// Convenience: build an ArtifactRef directly from an `ArtifactPreview`
    /// returned by the Rust core (which already carries `reference_tag_*`
    /// fields populated for whichever artifact type it represents).
    init?(preview: ArtifactPreview) {
        let kind = UInt16(preview.referenceKind) ?? 0
        let value = preview.referenceTagValue
        guard !value.isEmpty else { return nil }
        switch preview.referenceTagName.lowercased() {
        case "a": self = .article(addr: value, kind: kind == 0 ? 30023 : kind)
        case "e": self = .event(id: value, kind: kind)
        case "i": self = .external(id: value, kind: kind)
        default: return nil
        }
    }
}
