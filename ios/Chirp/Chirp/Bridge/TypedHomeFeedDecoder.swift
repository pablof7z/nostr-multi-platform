import Foundation
import FlatBuffers

/// Decodes the typed `nmp.feed.home` sidecar from a FlatBuffers `NOFS` buffer
/// (ADR-0038 Stage T3) into the SAME `ChirpTimelineSnapshot` model the generic
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
/// CONSUMER STATUS — this decoder is currently exercised only by
/// `ChirpTests/OpFeedDecoderTests.swift`. It is intentionally NOT wired into
/// `KernelModel.modularTimeline` yet: the typed `TimelineEventCard` carries its
/// content tree as embedded `NFCT` bytes and its relation counts as a typed
/// sub-table, but iOS has no Swift `NFCT` decoder — so `card.contentTree` /
/// `card.relationCounts` cannot be filled here (see the `nil` fields in
/// `makeCard`). Flipping the runtime preference (prefer typed, fall back to
/// generic) is a follow-up that first needs render-completeness for those two
/// fields — either a Rust `decode_op_feed_snapshot`→JSON FFI helper (mirrors
/// the chirp-tui T2 approach) or a Swift `NFCT` decoder. Until then the runtime
/// always reads the generic `Value` path, matching the prior NFTS pilot, which
/// was likewise decoder-only.
enum TypedHomeFeedDecoder {
    /// Projection key as published by the kernel (`TypedProjection.key`).
    static let projectionKey = "nmp.feed.home"
    /// Schema id carried in `TypedPayload.schema_id` for the NOFS wire.
    static let schemaId = "nmp.nip01.opfeed"
    /// FlatBuffers `file_identifier` for `OpFeedSnapshot`.
    static let fileIdentifier = "NOFS"
    /// FlatBuffers `file_identifier` for the embedded `FeedWindow` sub-buffer.
    static let feedWindowFileIdentifier = "NFWM"

    /// Extract and decode the `nmp.feed.home` typed payload from a set of typed
    /// projection envelopes lifted off a snapshot frame.
    static func decode(from projections: [TypedProjectionEnvelope]) -> ChirpTimelineSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == projectionKey && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NOFS` FlatBuffers buffer into the Swift feed model.
    ///
    /// Uses `getCheckedRoot` so the FlatBuffers runtime verifies both the file
    /// identifier and structural integrity in one pass; any failure surfaces as
    /// `nil` rather than a thrown error, honouring the graceful-fallback
    /// contract.
    static func decode(bytes: Data) -> ChirpTimelineSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        guard let snapshot: nmp_nip01_OpFeedSnapshot = try? getCheckedRoot(
            byteBuffer: &buffer,
            fileId: fileIdentifier
        ) else {
            return nil
        }

        let cards = snapshot.cards.map(makeRootCard)
        let page = snapshot.hasPage ? decodePage(snapshot) : nil
        return ChirpTimelineSnapshot(cards: cards, page: page)
    }

    // ── Card mapping ─────────────────────────────────────────────────────────

    private static func makeRootCard(_ root: nmp_nip01_RootCard) -> ChirpRootCard {
        ChirpRootCard(
            card: makeCard(root.card),
            attribution: root.attribution.map(makeAttribution)
        )
    }

    private static func makeCard(_ card: nmp_nip01_TimelineEventCard?) -> ChirpEventCard {
        ChirpEventCard(
            id: card?.id ?? "",
            authorPubkey: card?.authorPubkey ?? "",
            kind: card?.kind ?? 0,
            createdAt: card?.createdAt ?? 0,
            content: card?.content ?? "",
            // The typed card carries its content tree as embedded NFCT bytes
            // (`content_tree_bytes`); iOS has no Swift NFCT decoder, so this
            // stays nil here. The generic `Value` path fills it from JSON. See
            // the file header — render-completeness for the typed path is a
            // follow-up. The field is Optional in the model, so nil is valid.
            contentTree: nil,
            // The typed card carries relation counts as a typed sub-table; the
            // Swift model decodes them from JSON on the generic path. Left nil
            // here for the same reason as `contentTree`.
            relationCounts: nil,
            // ADR-0032: `has_*` companion bool distinguishes "absent (no kind:0
            // yet)" from "present empty string".
            authorDisplayName: optionalString(card?.authorDisplayName, present: card?.hasAuthorDisplayName ?? false),
            authorPictureUrl: optionalString(card?.authorPictureUrl, present: card?.hasAuthorPictureUrl ?? false),
            contentPreview: card?.contentPreview ?? ""
        )
    }

    private static func makeAttribution(_ entry: nmp_nip01_ReplyAttribution) -> ChirpReplyAttribution {
        ChirpReplyAttribution(
            authorPubkey: entry.authorPubkey ?? "",
            authorDisplayName: optionalString(entry.authorDisplayName, present: entry.hasAuthorDisplayName),
            authorPictureUrl: optionalString(entry.authorPictureUrl, present: entry.hasAuthorPictureUrl),
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
        guard let window: nmp_feed_FeedWindow = try? getCheckedRoot(
            byteBuffer: &windowBuffer,
            fileId: feedWindowFileIdentifier
        ), let page = window.page else {
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

    /// Honour the schema's `has_*` companion bool: an absent field
    /// (`present == false`) is `nil` even when the string accessor returns "".
    private static func optionalString(_ value: String?, present: Bool) -> String? {
        present ? value : nil
    }
}
