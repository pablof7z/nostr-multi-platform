import FlatBuffers
import Foundation

// HAND-WRITTEN glue for the typed `refs.event.envelopes` sidecar (issue #1283
// Phase 1), kept in this sibling file rather than appended to
// `TypedProjectionGlue.swift` because that file is at its file-size baseline
// (AGENTS.md anti-cheat: split rather than raise a baseline).
//
// This is the half of the embed decode that maps the `flatc --swift` reader
// (`nmp_embed_RefEventEnvelopes`, decoded by the GENERATED
// `TypedRefEventEnvelopesDecoder`) to the Chirp domain
// `[String: EmbeddedEventEnvelope]`. The kind-dispatch + tag/JSON parsing that
// used to live in `EmbedHost.resolve()` is DELETED — the Rust resolver
// (`nmp_content::resolve_embed_projection`) already did it on the kernel side,
// so this glue is a pure field copy + enum re-tagging (D0 thin-shell: zero
// resolution logic in Swift).
extension TypedProjectionGlue {
    // MARK: refs.event.envelopes → [String: EmbeddedEventEnvelope]

    /// Map the typed `refs.event.envelopes` sidecar (`NEMB` /
    /// `nmp_embed_RefEventEnvelopes`) to the `[String: EmbeddedEventEnvelope]`
    /// the derived projection yields. FlatBuffers has no map type, so the
    /// producer flattens the `primary_id -> envelope` map to a key-sorted vector;
    /// this rebuilds the dictionary keyed by `primaryId`.
    static func refEventEnvelopes(
        _ reader: nmp_embed_RefEventEnvelopes
    ) -> [String: EmbeddedEventEnvelope] {
        reader.entries.reduce(into: [String: EmbeddedEventEnvelope]()) { out, entry in
            let primaryID = entry.primaryId ?? ""
            guard !primaryID.isEmpty, let projection = mapProjection(entry.projection) else {
                return
            }
            out[primaryID] = EmbeddedEventEnvelope(
                uri: entry.uri ?? "",
                primaryId: primaryID,
                depth: entry.depth,
                maxDepth: entry.maxDepth,
                projection: projection,
                collapsed: entry.collapsed,
                collapseReason: entry.hasCollapseReason ? (entry.collapseReason ?? "") : nil
            )
        }
    }

    /// Re-tag the kind-discriminated wire projection into the Swift
    /// `EmbedKindProjection` enum. The `kind` discriminant selects exactly one
    /// populated payload table (the schema mirrors the Rust enum). A missing
    /// payload for a kind is a malformed buffer → `nil` (the envelope is
    /// dropped rather than rendered blank).
    private static func mapProjection(
        _ wire: nmp_embed_EmbedKindProjection?
    ) -> EmbedKindProjection? {
        guard let wire else { return nil }
        switch wire.kind {
        case .shortnote:
            guard let p = wire.shortNote else { return nil }
            return .shortNote(ShortNoteProjection(
                id: p.id ?? "",
                authorPubkey: p.authorPubkey ?? "",
                authorDisplayName: p.hasAuthorDisplayName ? (p.authorDisplayName ?? "") : nil,
                authorPictureUrl: p.hasAuthorPictureUrl ? (p.authorPictureUrl ?? "") : nil,
                createdAt: p.createdAt,
                content: plainText(fromContentTree: p.contentTree),
                mediaUrls: p.mediaUrls.map { $0 ?? "" }
            ))
        case .article:
            guard let p = wire.article else { return nil }
            return .article(ArticleProjection(
                id: p.id ?? "",
                authorPubkey: p.authorPubkey ?? "",
                authorDisplayName: p.hasAuthorDisplayName ? (p.authorDisplayName ?? "") : nil,
                authorPictureUrl: p.hasAuthorPictureUrl ? (p.authorPictureUrl ?? "") : nil,
                createdAt: p.createdAt,
                title: p.hasTitle ? (p.title ?? "") : nil,
                summary: p.hasSummary ? (p.summary ?? "") : nil,
                heroImageUrl: p.hasHeroImageUrl ? (p.heroImageUrl ?? "") : nil,
                dTag: p.dTag ?? "",
                content: plainText(fromContentTree: p.contentTree)
            ))
        case .highlight:
            guard let p = wire.highlight else { return nil }
            return .highlight(HighlightProjection(
                id: p.id ?? "",
                authorPubkey: p.authorPubkey ?? "",
                authorDisplayName: p.hasAuthorDisplayName ? (p.authorDisplayName ?? "") : nil,
                createdAt: p.createdAt,
                highlightedText: p.highlightedText ?? "",
                sourceEventId: p.hasSourceEventId ? (p.sourceEventId ?? "") : nil,
                sourceEventAddr: p.hasSourceEventAddr ? (p.sourceEventAddr ?? "") : nil,
                sourceUrl: p.hasSourceUrl ? (p.sourceUrl ?? "") : nil,
                context: p.hasContext ? (p.context ?? "") : nil
            ))
        case .profile:
            guard let p = wire.profile else { return nil }
            return .profile(ProfileProjection(
                pubkey: p.pubkey ?? "",
                displayName: p.hasDisplayName ? (p.displayName ?? "") : nil,
                pictureUrl: p.hasPictureUrl ? (p.pictureUrl ?? "") : nil,
                about: p.hasAbout ? (p.about ?? "") : nil,
                nip05: p.hasNip05 ? (p.nip05 ?? "") : nil,
                lud16: p.hasLud16 ? (p.lud16 ?? "") : nil,
                bannerUrl: p.hasBannerUrl ? (p.bannerUrl ?? "") : nil
            ))
        case .unknown:
            guard let p = wire.unknown else { return nil }
            return .unknown(UnknownProjection(
                kind: p.kind,
                authorPubkey: p.authorPubkey ?? "",
                authorDisplayName: p.hasAuthorDisplayName ? (p.authorDisplayName ?? "") : nil,
                authorPictureUrl: p.hasAuthorPictureUrl ? (p.authorPictureUrl ?? "") : nil,
                createdAt: p.createdAt,
                content: p.content ?? "",
                tags: p.tags.map { row in row.values.map { $0 ?? "" } },
                altText: p.hasAltText ? (p.altText ?? "") : nil
            ))
        }
    }

    // MARK: - Wire helpers

    /// Copy a FlatBuffers `[ubyte]` content-tree sub-buffer into `Data`, decode
    /// it (`NFCT` → `ContentTreeWire`), and flatten its text into the plain
    /// `content` string the default embed renderers display. NOT a re-parse: the
    /// tree was produced by the Rust resolver; this is a text concatenation of an
    /// already-resolved structure (D0-clean — no kind dispatch, no tag/JSON
    /// parsing). An absent/empty tree yields an empty string.
    private static func plainText(fromContentTree vector: FlatbufferVector<UInt8>) -> String {
        let bytes = Data(vector.map { $0 })
        guard !bytes.isEmpty,
              let tree = TypedHomeFeedDecoder.decodeContentTree(fromBytes: bytes)
        else {
            return ""
        }
        return tree.plainText()
    }
}

// Plain-text flattening of an already-resolved `ContentTreeWire`. This is NOT a
// content parser (the Rust resolver produced the tree); it concatenates the
// tree's textual leaves so the default embed renderers — which display a single
// `Text(note.content)` — keep working after the in-Swift resolver is deleted.
// Kept here (not in `ContentTreeWire.swift`, which is over the 300-LOC soft cap)
// alongside its sole caller.
extension ContentTreeWire {
    /// Concatenate the tree's text leaves in root order into a single string,
    /// separating block-level nodes with newlines. Inline text/code/url/hashtag
    /// leaves contribute their literal value; structural nodes recurse over their
    /// children. Media/emoji/invoice/placeholder leaves contribute nothing (the
    /// media URLs are surfaced separately via `ShortNoteProjection.mediaUrls`).
    func plainText() -> String {
        var pieces: [String] = []
        for root in roots {
            appendText(ofNodeAt: root, into: &pieces)
        }
        return pieces.joined().trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func appendText(ofNodeAt index: UInt32, into pieces: inout [String]) {
        guard let node = node(at: index) else { return }
        switch node {
        case .text(let value), .url(let value), .inlineCode(let value):
            pieces.append(value)
        case .hashtag(let tag):
            pieces.append("#\(tag)")
        case .mention(let uri), .eventRef(let uri):
            pieces.append(uri.uri)
        case .codeBlock(_, let body):
            pieces.append(body)
            pieces.append("\n")
        case .softBreak:
            pieces.append(" ")
        case .hardBreak:
            pieces.append("\n")
        case .heading(_, let children),
             .paragraph(let children),
             .blockQuote(let children),
             .emphasis(let children),
             .strong(let children),
             .link(let children, _):
            for child in children { appendText(ofNodeAt: child, into: &pieces) }
            if case .paragraph = node { pieces.append("\n") }
            if case .heading = node { pieces.append("\n") }
        case .list(_, let items):
            for item in items {
                for child in item { appendText(ofNodeAt: child, into: &pieces) }
                pieces.append("\n")
            }
        case .media, .emoji, .invoice, .image, .rule, .placeholder:
            break
        }
    }
}
