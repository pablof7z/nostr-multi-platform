import Foundation
import FlatBuffers

/// Decodes the typed `nmp.feed.home` sidecar from a FlatBuffers `NOFS` buffer
/// (ADR-0038 Stage T4) into the SAME `OpFeedSnapshot` model the generic
/// `Value` projection path produces, so `HomeFeedView` renders either source
/// identically.
///
/// ADR-0037 introduced typed FlatBuffers runtime projections carried alongside
/// the generic snapshot `payload`. The authorized pilot is `nmp.feed.home`,
/// whose OP-centric view is the nmp-feed `RootFeedSnapshot<TimelineEventCard,
/// Nip10ReplyAttribution>` (`schema_id = "nmp.nip01.opfeed"`, `file_identifier
/// = "NOFS"`). The retired NFTS descriptor (`nmp.nip01.timeline`) is no longer
/// preferred — an `NFTS`-tagged entry is treated as unrecognized and falls
/// through to the generic projection (ADR-0037 Commitment 4).
///
/// Every entry point falls back gracefully — it returns `nil` when the
/// projection is absent, carries the wrong schema id, or cannot be verified as
/// a well-formed `NOFS` buffer. Hosts treat `nil` as "no typed feed available"
/// and keep rendering the generic snapshot.
///
/// CONSUMER STATUS — the typed path is now LIVE. `TypedHomeFeedDecoder` is
/// wired into `KernelBridge.decodeFlatBuffer` as the PREFERRED decode path;
/// `KernelModel.modularTimeline` returns the typed result when available,
/// falling back to the generic `Value` path when the typed decode returns nil
/// (ADR-0037 Commitment 4). The `NFCT` content-tree sub-buffer and the
/// `NoteRelationCounts` typed sub-table are both fully decoded in Swift using
/// the generated `nmp_content_*` FlatBuffers accessors.
enum TypedHomeFeedDecoder {
    /// Projection key as published by the kernel (`TypedProjection.key`).
    static let projectionKey = "nmp.feed.home"
    /// Schema id carried in `TypedPayload.schema_id` for the NOFS wire.
    static let schemaId = "nmp.nip01.opfeed"
    /// FlatBuffers `file_identifier` for `OpFeedSnapshot`.
    static let fileIdentifier = "NOFS"
    /// FlatBuffers `file_identifier` for the embedded `FeedWindow` sub-buffer.
    static let feedWindowFileIdentifier = "NFWM"
    /// FlatBuffers `file_identifier` for the embedded `ContentTreeWire` sub-buffer.
    static let contentTreeFileIdentifier = "NFCT"

    /// Extract and decode the `nmp.feed.home` typed payload from a set of typed
    /// projection envelopes lifted off a snapshot frame.
    static func decode(from projections: [TypedProjectionEnvelope]) -> OpFeedSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == projectionKey && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NOFS` FlatBuffers buffer into the Swift feed model.
    ///
    /// Uses `getRoot` (unchecked) because this buffer arrives over a trusted
    /// in-process FFI boundary from the Rust kernel — running the O(N)
    /// FlatBuffers Verifier on every 4 Hz frame is pure wasted CPU. The
    /// `!bytes.isEmpty` guard above is the only presence check needed; a
    /// gross wiring error would surface as empty/default field values from
    /// the glue layer rather than a crash.
    static func decode(bytes: Data) -> OpFeedSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let snapshot: nmp_nip01_OpFeedSnapshot = getRoot(byteBuffer: &buffer)

        let cards = snapshot.cards.map(makeRootCard)
        let page = snapshot.hasPage ? decodePage(snapshot) : nil
        return OpFeedSnapshot(cards: cards, page: page)
    }

    // ── Card mapping ─────────────────────────────────────────────────────────

    private static func makeRootCard(_ root: nmp_nip01_RootCard) -> ChirpRootCard {
        ChirpRootCard(
            card: makeCard(root.card),
            attribution: root.attribution.map(makeAttribution)
        )
    }

    private static func makeCard(_ card: nmp_nip01_TimelineEventCard?) -> ChirpEventCard {
        // Decode the embedded NFCT content-tree sub-buffer. Returns nil when
        // the buffer is absent or malformed — ChirpEventCard.contentTree is
        // Optional, so nil is valid and the renderer falls back to plain content.
        let contentTree: ContentTreeWire? = card.flatMap { decodeContentTree($0) }

        // Map the typed NoteRelationCounts sub-table. Absent == nil.
        let relationCounts: NoteRelationCounts? = card?.relationCounts.flatMap(makeRelationCounts)

        return ChirpEventCard(
            id: card?.id ?? "",
            authorPubkey: card?.authorPubkey ?? "",
            kind: card?.kind ?? 0,
            createdAt: card?.createdAt ?? 0,
            content: card?.content ?? "",
            contentTree: contentTree,
            relationCounts: relationCounts,
            // ADR-0032: `has_*` companion bool distinguishes "absent (no kind:0
            // yet)" from "present empty string".
            authorDisplayName: optionalString(card?.authorDisplayName, present: card?.hasAuthorDisplayName ?? false),
            authorPictureUrl: optionalString(card?.authorPictureUrl, present: card?.hasAuthorPictureUrl ?? false),
            contentPreview: card?.contentPreview ?? "",
            relayProvenance: card?.relayProvenance.compactMap { $0 } ?? [],
            // Typed NIP-18 repost signal (D0): the kernel stamps `reposted_by`
            // when a kind:6 repost superseded the original note. We must read
            // this typed table rather than re-derive `kind == 6` in the view —
            // a repost card carries the *original* note's kind, not 6.
            isRepost: card?.repostedBy != nil
        )
    }

    private static func makeAttribution(_ entry: nmp_nip01_ReplyAttribution) -> ChirpReplyAttribution {
        // ADR-0032 / #1493: flat mirrors removed; read nested authorDisplay.
        let display = entry.authorDisplay
        return ChirpReplyAttribution(
            authorPubkey: entry.authorPubkey ?? "",
            authorDisplayName: optionalString(display?.name, present: display?.hasName ?? false),
            authorPictureUrl: optionalString(display?.pictureUrl, present: display?.hasPictureUrl ?? false),
            replyEventId: entry.replyEventId ?? "",
            replyCreatedAt: entry.replyCreatedAt
        )
    }

    // ── Feed-window (NFWM) sub-buffer → page ──────────────────────────────────

    /// Decode the embedded `feed_window_bytes` (`NFWM`) sub-buffer and map its
    /// `FeedPage` to the Swift `TimelineWindowPage` the renderer paginates on.
    /// Returns `nil` when the window is absent, malformed, or carries no page
    /// (the generic decoder likewise ignores `metrics`, so this maps page only).
    private static func decodePage(_ snapshot: nmp_nip01_OpFeedSnapshot) -> TimelineWindowPage? {
        // `withUnsafePointerToFeedWindowBytes` returns `T?` where the closure
        // result is itself `Data?`, so the call yields `Data??`. A `nil` window
        // (absent vector) or an empty slice both collapse to "no page".
        let windowData: Data? = snapshot.withUnsafePointerToFeedWindowBytes { pointer, count -> Data? in
            guard count > 0, let base = pointer.baseAddress else { return nil }
            return Data(bytes: base, count: count)
        } ?? nil
        guard let data = windowData, !data.isEmpty else { return nil }
        var windowBuffer = ByteBuffer(data: data)
        // Trusted in-process sub-buffer (embedded within a kernel-produced NOFS
        // frame); skip the O(N) Verifier walk.
        let window: nmp_feed_FeedWindow = getRoot(byteBuffer: &windowBuffer)
        guard let page = window.page else {
            return nil
        }
        // Explicit closure rather than `Optional.flatMap` to avoid any
        // Sequence/Optional overload-resolution ambiguity in the Swift FB types.
        let cursor: TimelineWindowCursor? = {
            guard let raw = page.nextCursor, let id = raw.id else { return nil }
            return TimelineWindowCursor(createdAt: raw.createdAt, id: id)
        }()
        return TimelineWindowPage(
            limit: UInt(clamping: page.limit),
            nextCursor: cursor,
            hasMore: page.hasMore,
            totalBlocks: UInt(clamping: page.totalBlocks)
        )
    }

    // ── NFCT content-tree sub-buffer → ContentTreeWire ────────────────────────

    /// Decode a raw `NFCT` FlatBuffers buffer directly into `ContentTreeWire`.
    ///
    /// Exposed as internal (not private) so tests can call it directly without
    /// needing a wrapping NOFS snapshot. For production use, prefer
    /// `decodeContentTree(_: nmp_nip01_TimelineEventCard)`.
    static func decodeContentTree(fromBytes bytes: Data) -> ContentTreeWire? {
        guard !bytes.isEmpty else { return nil }
        var treeBuffer = ByteBuffer(data: bytes)
        // Trusted in-process sub-buffer; skip the O(N) Verifier walk.
        let root: nmp_content_ContentTreeWire = getRoot(byteBuffer: &treeBuffer)
        let nodes = root.nodes.compactMap(decodeWireNode)
        let roots: [UInt32] = root.roots.map { $0 }
        let mode = renderModeString(root.mode)
        return ContentTreeWire(nodes: nodes, roots: roots, mode: mode)
    }

    /// Decode the embedded `content_tree_bytes` (`NFCT`) sub-buffer carried on
    /// a `TimelineEventCard` into the Swift `ContentTreeWire` model.
    ///
    /// Mirrors the `decodePage` pattern: extract the raw bytes via
    /// `withUnsafePointerToContentTreeBytes`, wrap in a `ByteBuffer`, then call
    /// `getRoot` (unchecked) to obtain the reader struct. The buffer is
    /// trusted (embedded within a kernel-produced NOFS frame); the O(N)
    /// FlatBuffers Verifier walk is skipped. Returns `nil` when the sub-buffer
    /// is absent — `contentTree` is Optional on `ChirpEventCard` so nil is
    /// valid and the renderer falls back to the plain `content` string.
    ///
    /// Raw values only — no display helpers (D11, display-separation audit
    /// 2026-05-25).
    static func decodeContentTree(_ card: nmp_nip01_TimelineEventCard) -> ContentTreeWire? {
        let treeData: Data? = card.withUnsafePointerToContentTreeBytes { pointer, count -> Data? in
            guard count > 0, let base = pointer.baseAddress else { return nil }
            return Data(bytes: base, count: count)
        } ?? nil
        guard let data = treeData, !data.isEmpty else { return nil }
        var treeBuffer = ByteBuffer(data: data)
        // Trusted in-process sub-buffer; skip the O(N) Verifier walk.
        let root: nmp_content_ContentTreeWire = getRoot(byteBuffer: &treeBuffer)
        let nodes = root.nodes.compactMap(decodeWireNode)
        let roots: [UInt32] = root.roots.map { $0 }
        let mode = renderModeString(root.mode)
        return ContentTreeWire(nodes: nodes, roots: roots, mode: mode)
    }

    /// Map the FlatBuffers `RenderMode` enum to the string the `ContentTreeWire`
    /// model carries. The JSON serde repr uses PascalCase (no `rename_all`):
    ///   Auto=0 → "Auto", Markdown=1 → "Markdown", Text=2 → "Plain"
    /// (the FB enum value `text` corresponds to Rust `RenderMode::Plain`).
    private static func renderModeString(_ mode: nmp_content_RenderMode) -> String? {
        switch mode {
        case .auto:     return "Auto"
        case .markdown: return "Markdown"
        case .text:     return "Plain"
        default:        return nil
        }
    }

    /// Decode one `WireNode` table into the Swift `NostrWireNode` enum.
    /// On any missing required field or unknown kind, returns nil (the node is
    /// silently dropped rather than aborting the whole tree decode — D1 partial).
    private static func decodeWireNode(_ node: nmp_content_WireNode) -> NostrWireNode? {
        switch node.kind {
        case .text:
            guard let text = node.text else { return nil }
            return .text(text)

        case .mention:
            guard let uri = decodeNostrUri(node.nostrUri) else { return nil }
            return .mention(uri)

        case .eventref:
            guard let uri = decodeNostrUri(node.nostrUri) else { return nil }
            return .eventRef(uri)

        case .hashtag:
            guard let tag = node.tag else { return nil }
            return .hashtag(tag)

        case .url:
            guard let url = node.url else { return nil }
            return .url(url)

        case .media:
            let urls = node.mediaUrls.compactMap { $0 }
            let kind = decodeMediaKind(node.mediaKind)
            return .media(urls: urls, kind: kind)

        case .emoji:
            guard let shortcode = node.shortcode else { return nil }
            return .emoji(shortcode: shortcode, url: node.emojiUrl)

        case .invoice:
            guard let payload = node.invoicePayload else { return nil }
            let invoice = decodeInvoice(kind: node.invoiceKind, payload: payload)
            return .invoice(invoice)

        case .heading:
            let children: [UInt32] = node.children.map { $0 }
            return .heading(level: node.level, children: children)

        case .paragraph:
            let children: [UInt32] = node.children.map { $0 }
            return .paragraph(children: children)

        case .blockquote:
            let children: [UInt32] = node.children.map { $0 }
            return .blockQuote(children: children)

        case .codeblock:
            guard let body = node.text else { return nil }
            return .codeBlock(info: node.codeInfo, body: body)

        case .list:
            let items: [[UInt32]] = node.listItems.map { item in
                item.children.map { $0 }
            }
            // ordered_start == -1 (default) means unordered (None).
            let orderedStart: UInt64? = node.orderedStart == -1 ? nil : UInt64(clamping: node.orderedStart)
            return .list(orderedStart: orderedStart, items: items)

        case .rule:
            return .rule

        case .emphasis:
            let children: [UInt32] = node.children.map { $0 }
            return .emphasis(children: children)

        case .strong:
            let children: [UInt32] = node.children.map { $0 }
            return .strong(children: children)

        case .inlinecode:
            guard let code = node.text else { return nil }
            return .inlineCode(code)

        case .link:
            let children: [UInt32] = node.children.map { $0 }
            return .link(children: children, href: node.href)

        case .image:
            guard let alt = node.alt else { return nil }
            // Image src is stored in the `url` field (Rust encoder: `args.url = src`).
            return .image(alt: alt, title: node.imgTitle, src: node.url)

        case .softbreak:
            return .softBreak

        case .hardbreak:
            return .hardBreak

        case .placeholder:
            let reason = decodePlaceholderReason(node.placeholderReason)
            return .placeholder(reason: reason)

        default:
            // Forward-compat: any unknown kind collapses to a depth_limit
            // placeholder rather than failing the whole decode (D1).
            return .placeholder(reason: .depthLimit)
        }
    }

    private static func decodeNostrUri(_ fbUri: nmp_content_WireNostrUri?) -> NostrWireUri? {
        guard let fbUri,
              let uri = fbUri.uri,
              let primaryId = fbUri.primaryId else { return nil }
        let kind: NostrWireUriKind
        switch fbUri.kind {
        case .profile:  kind = .profile
        case .event:    kind = .event
        case .address:  kind = .address
        default:        kind = .profile
        }
        let relays: [String] = fbUri.relays.compactMap { $0 }
        // event_kind sentinel: u32::MAX == UInt32.max means None.
        let eventKind: UInt32? = fbUri.eventKind == UInt32.max ? nil : fbUri.eventKind
        return NostrWireUri(
            uri: uri,
            kind: kind,
            primaryId: primaryId,
            relays: relays,
            author: fbUri.author,
            eventKind: eventKind
        )
    }

    /// MediaKind discriminant: Image=0, Video=1, Audio=2 (from Rust encoder).
    private static func decodeMediaKind(_ v: UInt8) -> NostrMediaKind {
        switch v {
        case 0: return .image
        case 1: return .video
        case 2: return .audio
        default: return .image
        }
    }

    /// InvoiceKind discriminant: Bolt11=0, Bolt12=1, Cashu=2 (from Rust encoder).
    private static func decodeInvoice(kind: UInt8, payload: String) -> NostrWireInvoice {
        switch kind {
        case 0: return .bolt11(payload)
        case 1: return .bolt12(payload)
        case 2: return .cashu(payload)
        default: return .bolt11(payload)
        }
    }

    private static func decodePlaceholderReason(_ reason: nmp_content_PlaceholderReason) -> NostrWirePlaceholderReason {
        switch reason {
        case .depthlimit:    return .depthLimit
        case .unresolveduri: return .unresolvedUri
        default:             return .depthLimit
        }
    }

    // ── Relation counts ───────────────────────────────────────────────────────

    private static func makeRelationCounts(_ fb: nmp_nip01_NoteRelationCounts) -> NoteRelationCounts? {
        guard let replies = fb.replies,
              let reactions = fb.reactions,
              let reposts = fb.reposts,
              let zaps = fb.zaps else { return nil }
        return NoteRelationCounts(
            replies: makeRelationCount(replies),
            reactions: makeRelationCount(reactions),
            reposts: makeRelationCount(reposts),
            zaps: makeRelationCount(zaps)
        )
    }

    private static func makeRelationCount(_ fb: nmp_nip01_RelationCount) -> RelationCount {
        switch fb.state {
        case .known:   return .known(fb.count)
        case .loading: return .loading
        default:       return .loading
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Honour the schema's `has_*` companion bool: an absent field
    /// (`present == false`) is `nil` even when the string accessor returns "".
    private static func optionalString(_ value: String?, present: Bool) -> String? {
        present ? value : nil
    }
}
