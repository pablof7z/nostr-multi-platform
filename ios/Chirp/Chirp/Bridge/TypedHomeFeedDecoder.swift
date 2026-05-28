import Foundation
import FlatBuffers

/// Decodes the typed `nmp.feed.home` sidecar from a FlatBuffers NFTS buffer.
///
/// ADR-0037 introduced typed FlatBuffers runtime projections carried alongside
/// the generic snapshot `payload`. The authorized pilot is `nmp.feed.home`,
/// whose full assembled view is the nmp-nip01 `ModularTimelineSnapshot`
/// (`schema_id = "nmp.nip01.timeline"`, `file_identifier = "NFTS"`).
///
/// Every entry point falls back gracefully — it returns `nil` when the
/// projection is absent, carries the wrong schema id, or cannot be verified as
/// a well-formed NFTS buffer. Hosts treat `nil` as "no typed feed available"
/// and keep rendering the generic snapshot.
enum TypedHomeFeedDecoder {
    /// Projection key as published by the kernel (`TypedProjection.key`).
    static let projectionKey = "nmp.feed.home"
    /// Schema id carried in `TypedPayload.schema_id` for the NFTS wire.
    static let schemaId = "nmp.nip01.timeline"
    /// FlatBuffers `file_identifier` for `ModularTimelineSnapshot`.
    static let fileIdentifier = "NFTS"

    /// Extract and decode the `nmp.feed.home` typed payload from a set of typed
    /// projection envelopes lifted off a snapshot frame.
    static func decode(from projections: [TypedProjectionEnvelope]) -> TypedHomeFeedSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == projectionKey && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw NFTS FlatBuffers buffer into a Swift view of the feed.
    ///
    /// Uses `getCheckedRoot` so the FlatBuffers runtime verifies both the file
    /// identifier and structural integrity in one pass; any failure surfaces as
    /// `nil` rather than a thrown error, honouring the graceful-fallback
    /// contract.
    static func decode(bytes: Data) -> TypedHomeFeedSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        guard let snapshot: nmp_nip01_ModularTimelineSnapshot = try? getCheckedRoot(
            byteBuffer: &buffer,
            fileId: fileIdentifier
        ) else {
            return nil
        }

        let blocks = snapshot.blocks.map(makeBlock)
        let cards = snapshot.cards.compactMap(makeCard)
        return TypedHomeFeedSnapshot(
            blocks: blocks,
            cards: cards,
            schemaVersion: snapshot.schemaVersion
        )
    }

    private static func makeBlock(
        _ entry: nmp_nip01_TimelineBlockEntry
    ) -> TypedHomeFeedSnapshot.Block {
        switch entry.kind {
        case .standalone:
            let id = entry.standaloneId
            return TypedHomeFeedSnapshot.Block(
                kind: .standalone,
                eventIds: id.map { [$0] } ?? [],
                hasGap: false,
                rootId: entry.standaloneRoot?.id
            )
        case .module:
            let eventIds = entry.moduleEventIds.compactMap { $0.id }
            return TypedHomeFeedSnapshot.Block(
                kind: .module,
                eventIds: eventIds,
                hasGap: entry.moduleHasGap,
                rootId: entry.moduleRoot?.id
            )
        }
    }

    private static func makeCard(
        _ card: nmp_nip01_TimelineEventCard
    ) -> TypedHomeFeedSnapshot.Card? {
        // A card without an id is unusable for diffing/rendering — drop it
        // rather than surface a placeholder.
        guard let id = card.id else { return nil }
        return TypedHomeFeedSnapshot.Card(
            id: id,
            authorPubkey: card.authorPubkey ?? "",
            authorDisplayName: optionalString(card.authorDisplayName, present: card.hasAuthorDisplayName),
            authorPictureUrl: optionalString(card.authorPictureUrl, present: card.hasAuthorPictureUrl),
            kind: card.kind,
            // Source timestamps are non-negative UNIX seconds; clamp the
            // UInt64 wire field into the Int64 host shape deliberately.
            createdAt: Int64(clamping: card.createdAt),
            content: card.content ?? "",
            contentPreview: card.contentPreview ?? ""
        )
    }

    /// Honour the schema's `has_*` companion bool: an absent field
    /// (`present == false`) is `nil` even when the string accessor returns "".
    private static func optionalString(_ value: String?, present: Bool) -> String? {
        present ? value : nil
    }
}

/// Swift view of a decoded home feed snapshot.
struct TypedHomeFeedSnapshot {
    struct Block {
        enum Kind { case standalone, module }
        let kind: Kind
        let eventIds: [String]
        let hasGap: Bool
        let rootId: String?
    }

    struct Card {
        let id: String
        let authorPubkey: String
        let authorDisplayName: String?
        let authorPictureUrl: String?
        let kind: UInt32
        let createdAt: Int64
        let content: String
        let contentPreview: String
    }

    let blocks: [Block]
    let cards: [Card]
    let schemaVersion: UInt32
}
