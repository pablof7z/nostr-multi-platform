struct NoteRenderContext: Equatable, Sendable {
    let eventCards: [String: ChirpEventCard]

    static let empty = NoteRenderContext(eventCards: [:])

    // ADR-0063 Lane E (#1671): inline mention labels are NO LONGER carried in
    // the render context. A whole-map profile dictionary threaded through every
    // row makes a single kind:0 update re-render the whole list (and broadcasts
    // via `@Published`). Mentions now read the per-key `KeyedRefCache` at render
    // time in `NoteContentView`, observed per-key so only the notes mentioning
    // the updated pubkey re-render.
    func contentTree(for row: NoteRowModel, fallback: ContentTreeWire?) -> ContentTreeWire? {
        if row.isRepost {
            return eventCards[row.id]?.contentTree
                ?? eventCards[row.navTargetId]?.contentTree
                ?? fallback
        }
        return fallback ?? eventCards[row.id]?.contentTree
    }
}

struct NoteRowModel: Equatable, Identifiable, Sendable {
    let id: String
    let authorPubkey: String
    let kind: UInt32
    let createdAt: UInt64
    let content: String
    let contentPreview: String
    let authorDisplayName: String?
    let authorPictureUrl: String?
    let isRepost: Bool
    let navTargetId: String
    let relayProvenance: [String]

    var relayCount: UInt32 { UInt32(relayProvenance.count) }

    init(
        id: String,
        authorPubkey: String,
        kind: UInt32,
        createdAt: UInt64,
        content: String,
        contentPreview: String,
        authorDisplayName: String?,
        authorPictureUrl: String?,
        isRepost: Bool,
        navTargetId: String,
        relayProvenance: [String]
    ) {
        self.id = id
        self.authorPubkey = authorPubkey
        self.kind = kind
        self.createdAt = createdAt
        self.content = content
        self.contentPreview = contentPreview
        self.authorDisplayName = authorDisplayName
        self.authorPictureUrl = authorPictureUrl
        self.isRepost = isRepost
        self.navTargetId = navTargetId
        self.relayProvenance = relayProvenance
    }

    init(card: ChirpEventCard) {
        self.init(
            id: card.id,
            authorPubkey: card.authorPubkey,
            kind: card.kind,
            createdAt: card.createdAt,
            content: card.content,
            contentPreview: card.contentPreview,
            authorDisplayName: card.authorDisplayName,
            authorPictureUrl: card.authorPictureUrl,
            isRepost: card.isRepost,
            navTargetId: card.id,
            relayProvenance: card.relayProvenance
        )
    }

    var renderedContent: String {
        content
    }

    func rendersIdentically(_ other: Self) -> Bool {
        id == other.id
            && authorPubkey == other.authorPubkey
            && authorDisplayName == other.authorDisplayName
            && authorPictureUrl == other.authorPictureUrl
            && content == other.content
            && contentPreview == other.contentPreview
            && createdAt == other.createdAt
            && isRepost == other.isRepost
            && kind == other.kind
            && navTargetId == other.navTargetId
            && relayProvenance == other.relayProvenance
    }
}

func shortEntity(_ value: String) -> String {
    guard value.count > 12 else { return value }
    return "\(value.prefix(8))…\(value.suffix(4))"
}

extension ContentTreeWire {
    /// Hex pubkeys of every `nostr:npub…` / `nprofile…` profile mention in this
    /// content tree, in stable arena order, de-duplicated.
    ///
    /// F-CR-00 claim-only invariant: a mention is an author-displaying surface,
    /// so the rendering view must claim each mentioned pubkey's kind:0 (mirror
    /// of `NostrAvatar`). Mentions render as inline `Text` runs inside a single
    /// concatenated `Text` (no per-mention SwiftUI view with its own lifecycle),
    /// so the claim is hoisted to the host view (`NoteContentView`) keyed off
    /// this list.
    var mentionPubkeys: [String] {
        var seen = Set<String>()
        var out: [String] = []
        for node in nodes {
            if case .mention(let uri) = node, uri.kind == .profile {
                let pk = uri.primaryId
                if !pk.isEmpty, seen.insert(pk).inserted {
                    out.append(pk)
                }
            }
        }
        return out
    }
}
