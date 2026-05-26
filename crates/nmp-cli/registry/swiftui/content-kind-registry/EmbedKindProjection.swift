import Foundation

/// Swift Codable mirror of the Rust `nmp_content::embed_projection::EmbedKindProjection`.
///
/// The Rust enum is serialized with `#[serde(tag = "variant", content = "data",
/// rename_all = "camelCase")]`, so JSON looks like:
///   { "variant": "article", "data": { "id": "…", "title": "…", … } }
///
/// Each variant is a typed payload the native renderer consumes verbatim; the
/// dispatch decision is `match projection` on the Swift side.
public enum EmbedKindProjection: Equatable {
    case shortNote(ShortNoteProjection)
    case article(ArticleProjection)
    case highlight(HighlightProjection)
    case profile(ProfileProjection)
    case unknown(UnknownProjection)
}

/// kind:1 short text note projection.
public struct ShortNoteProjection: Equatable {
    public let id: String
    public let authorPubkey: String
    public let authorDisplayName: String?
    public let authorPictureUrl: String?
    public let createdAt: UInt64
    /// Plain-text fallback for the content body. The Swift mirror does NOT
    /// re-implement the Rust content tokenizer; renderers read `content` as a
    /// plain string and let SwiftUI handle the layout.
    public let content: String
    public let mediaUrls: [String]

    public init(
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
public struct ArticleProjection: Equatable {
    public let id: String
    public let authorPubkey: String
    public let authorDisplayName: String?
    public let authorPictureUrl: String?
    public let createdAt: UInt64
    public let title: String?
    public let summary: String?
    public let heroImageUrl: String?
    public let dTag: String
    /// Plain-text body fallback (the Rust resolver also emits a content tree;
    /// the Swift gallery showcase renders title/summary/hero image only).
    public let content: String

    public init(
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
public struct HighlightProjection: Equatable {
    public let id: String
    public let authorPubkey: String
    public let authorDisplayName: String?
    public let createdAt: UInt64
    public let highlightedText: String
    public let sourceEventId: String?
    public let sourceEventAddr: String?
    public let sourceUrl: String?
    public let context: String?

    public init(
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
public struct ProfileProjection: Equatable {
    public let pubkey: String
    public let displayName: String?
    public let pictureUrl: String?
    public let about: String?
    public let nip05: String?
    public let lud16: String?
    public let bannerUrl: String?

    public init(
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
public struct UnknownProjection: Equatable {
    public let kind: UInt32
    public let authorPubkey: String
    public let authorDisplayName: String?
    public let authorPictureUrl: String?
    public let createdAt: UInt64
    public let content: String
    public let tags: [[String]]
    public let altText: String?

    public init(
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
public struct EmbeddedEventEnvelope: Equatable {
    /// The original nostr: URI (nevent1… / naddr1… / npub1…).
    public let uri: String
    /// Primary identifier: event-id hex for event-addressed refs, or
    /// `"kind:pubkey:d"` coordinate string for addressable events.
    public let primaryId: String
    /// Recursion guard state. Currently surfaced as defaults; reserved for
    /// nested-embed depth limits.
    public let depth: UInt8
    public let maxDepth: UInt8
    /// Kind-dispatched projection — drives which native renderer is chosen.
    public let projection: EmbedKindProjection
    /// Whether this embed should be collapsed (depth limit, cycle, unsupported).
    public let collapsed: Bool
    /// Optional machine-readable collapse reason.
    public let collapseReason: String?

    public init(
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
