import Foundation

/// Swift Codable mirror of the Rust `nmp_content::embed_projection::EmbedKindProjection`.
///
/// The Rust enum is serialized with `#[serde(tag = "variant", content = "data",
/// rename_all = "camelCase")]`, so JSON looks like:
///   { "variant": "article", "data": { "id": "…", "title": "…", … } }
///
/// Each variant is a typed payload the native renderer consumes verbatim; the
/// dispatch decision is `match projection` on the Swift side.
enum EmbedKindProjection: Equatable {
    case shortNote(ShortNoteProjection)
    case article(ArticleProjection)
    case highlight(HighlightProjection)
    case profile(ProfileProjection)
    case unknown(UnknownProjection)
}

extension EmbedKindProjection: Decodable {
    /// Rust wire shape: `{ "variant": "<camelCase>", "data": { … } }`.
    private enum CodingKeys: String, CodingKey { case variant, data }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let tag = try c.decode(String.self, forKey: .variant)
        switch tag {
        case "shortNote":
            self = .shortNote(try c.decode(ShortNoteProjection.self, forKey: .data))
        case "article":
            self = .article(try c.decode(ArticleProjection.self, forKey: .data))
        case "highlight":
            self = .highlight(try c.decode(HighlightProjection.self, forKey: .data))
        case "profile":
            self = .profile(try c.decode(ProfileProjection.self, forKey: .data))
        default:
            self = .unknown(try c.decode(UnknownProjection.self, forKey: .data))
        }
    }
}

/// kind:1 short text note projection.
struct ShortNoteProjection: Equatable, Decodable {
    let id: String
    let authorPubkey: String
    let authorDisplayName: String?
    let authorPictureUrl: String?
    let createdAt: UInt64
    /// Plain-text fallback for the content body. The Swift mirror does NOT
    /// re-implement the Rust content tokenizer; renderers read `content` as a
    /// plain string and let SwiftUI handle the layout.
    let content: String
    let mediaUrls: [String]

    /// Explicit CodingKeys bypass `convertFromSnakeCase` so that the Rust
    /// camelCase JSON (`authorPubkey`, `createdAt`, …) maps directly to the
    /// Swift camelCase properties without the strategy lowercasing them.
    enum CodingKeys: String, CodingKey {
        case id, authorPubkey, authorDisplayName, authorPictureUrl, createdAt, content, mediaUrls
    }

    init(
        id: String,
        authorPubkey: String,
        authorDisplayName: String? = nil,
        authorPictureUrl: String? = nil,
        createdAt: UInt64 = 0,
        content: String = "",
        mediaUrls: [String] = []
    ) {
        self.id = id
        self.authorPubkey = authorPubkey
        self.authorDisplayName = authorDisplayName
        self.authorPictureUrl = authorPictureUrl
        self.createdAt = createdAt
        self.content = content
        self.mediaUrls = mediaUrls
    }
}

/// kind:30023 long-form article projection (NIP-23).
struct ArticleProjection: Equatable, Decodable {
    let id: String
    let authorPubkey: String
    let authorDisplayName: String?
    let authorPictureUrl: String?
    let createdAt: UInt64
    let title: String?
    let summary: String?
    let heroImageUrl: String?
    let dTag: String
    /// Plain-text body fallback (the Rust resolver also emits a content tree;
    /// the Swift gallery showcase renders title/summary/hero image only).
    let content: String

    enum CodingKeys: String, CodingKey {
        case id, authorPubkey, authorDisplayName, authorPictureUrl, createdAt
        case title, summary, heroImageUrl, dTag, content
    }

    init(
        id: String,
        authorPubkey: String,
        authorDisplayName: String? = nil,
        authorPictureUrl: String? = nil,
        createdAt: UInt64 = 0,
        title: String? = nil,
        summary: String? = nil,
        heroImageUrl: String? = nil,
        dTag: String = "",
        content: String = ""
    ) {
        self.id = id
        self.authorPubkey = authorPubkey
        self.authorDisplayName = authorDisplayName
        self.authorPictureUrl = authorPictureUrl
        self.createdAt = createdAt
        self.title = title
        self.summary = summary
        self.heroImageUrl = heroImageUrl
        self.dTag = dTag
        self.content = content
    }
}

/// kind:9802 highlight projection (NIP-84).
struct HighlightProjection: Equatable, Decodable {
    let id: String
    let authorPubkey: String
    let authorDisplayName: String?
    let createdAt: UInt64
    let highlightedText: String
    let sourceEventId: String?
    let sourceEventAddr: String?
    let sourceUrl: String?
    let context: String?

    enum CodingKeys: String, CodingKey {
        case id, authorPubkey, authorDisplayName, createdAt
        case highlightedText, sourceEventId, sourceEventAddr, sourceUrl, context
    }

    init(
        id: String,
        authorPubkey: String,
        authorDisplayName: String? = nil,
        createdAt: UInt64 = 0,
        highlightedText: String = "",
        sourceEventId: String? = nil,
        sourceEventAddr: String? = nil,
        sourceUrl: String? = nil,
        context: String? = nil
    ) {
        self.id = id
        self.authorPubkey = authorPubkey
        self.authorDisplayName = authorDisplayName
        self.createdAt = createdAt
        self.highlightedText = highlightedText
        self.sourceEventId = sourceEventId
        self.sourceEventAddr = sourceEventAddr
        self.sourceUrl = sourceUrl
        self.context = context
    }
}

/// kind:0 profile metadata projection.
struct ProfileProjection: Equatable, Decodable {
    let pubkey: String
    let displayName: String?
    let pictureUrl: String?
    let about: String?
    let nip05: String?
    let lud16: String?
    let bannerUrl: String?

    enum CodingKeys: String, CodingKey {
        case pubkey, displayName, pictureUrl, about, nip05, lud16, bannerUrl
    }

    init(
        pubkey: String,
        displayName: String? = nil,
        pictureUrl: String? = nil,
        about: String? = nil,
        nip05: String? = nil,
        lud16: String? = nil,
        bannerUrl: String? = nil
    ) {
        self.pubkey = pubkey
        self.displayName = displayName
        self.pictureUrl = pictureUrl
        self.about = about
        self.nip05 = nip05
        self.lud16 = lud16
        self.bannerUrl = bannerUrl
    }
}

/// Fallback projection for kinds without a registered handler.
struct UnknownProjection: Equatable, Decodable {
    let kind: UInt32
    let authorPubkey: String
    let authorDisplayName: String?
    let authorPictureUrl: String?
    let createdAt: UInt64
    let content: String
    let tags: [[String]]
    let altText: String?

    enum CodingKeys: String, CodingKey {
        case kind, authorPubkey, authorDisplayName, authorPictureUrl, createdAt, content, tags, altText
    }

    init(
        kind: UInt32,
        authorPubkey: String,
        authorDisplayName: String? = nil,
        authorPictureUrl: String? = nil,
        createdAt: UInt64 = 0,
        content: String = "",
        tags: [[String]] = [],
        altText: String? = nil
    ) {
        self.kind = kind
        self.authorPubkey = authorPubkey
        self.authorDisplayName = authorDisplayName
        self.authorPictureUrl = authorPictureUrl
        self.createdAt = createdAt
        self.content = content
        self.tags = tags
        self.altText = altText
    }
}

/// Full envelope mirror of `nmp_content::embed_projection::EmbeddedEventEnvelope`.
struct EmbeddedEventEnvelope: Equatable, Decodable {
    /// The original nostr: URI (nevent1… / naddr1… / npub1…).
    let uri: String
    /// Primary identifier: event-id hex for event-addressed refs, or
    /// `"kind:pubkey:d"` coordinate string for addressable events.
    let primaryId: String
    /// Recursion guard state. Currently surfaced as defaults; reserved for
    /// nested-embed depth limits.
    let depth: UInt8
    let maxDepth: UInt8
    /// Kind-dispatched projection — drives which native renderer is chosen.
    let projection: EmbedKindProjection
    /// Whether this embed should be collapsed (depth limit, cycle, unsupported).
    let collapsed: Bool
    /// Optional machine-readable collapse reason.
    let collapseReason: String?

    init(
        uri: String,
        primaryId: String,
        depth: UInt8 = 0,
        maxDepth: UInt8 = 4,
        projection: EmbedKindProjection,
        collapsed: Bool = false,
        collapseReason: String? = nil
    ) {
        self.uri = uri
        self.primaryId = primaryId
        self.depth = depth
        self.maxDepth = maxDepth
        self.projection = projection
        self.collapsed = collapsed
        self.collapseReason = collapseReason
    }
}
