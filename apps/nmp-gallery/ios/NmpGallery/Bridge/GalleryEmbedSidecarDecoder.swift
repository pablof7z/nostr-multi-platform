import Foundation

enum GalleryEmbedSidecarDecoder {
    static func decode(_ data: Data) -> [String: EmbeddedEventEnvelope] {
        let reader = GalleryFlatBufferReader(data: data)
        guard reader.hasIdentifier("NEMB"),
              let root = try? reader.rootTable(),
              let entries = try? reader.tableVectorField(table: root, index: 0) else {
            return [:]
        }
        var result: [String: EmbeddedEventEnvelope] = [:]
        for entry in entries {
            guard let envelope = decodeEnvelope(entry, reader: reader) else { continue }
            result[envelope.primaryId] = envelope
        }
        return result
    }

    private static func decodeEnvelope(
        _ table: Int,
        reader: GalleryFlatBufferReader
    ) -> EmbeddedEventEnvelope? {
        guard let primaryId = try? reader.stringField(table: table, index: 0),
              let projectionTable = try? reader.tableField(table: table, index: 7),
              let projection = decodeProjection(projectionTable, reader: reader) else {
            return nil
        }
        let hasReason = ((try? reader.boolField(table: table, index: 5)) ?? false) == true
        let reason = hasReason ? (try? reader.stringField(table: table, index: 6)) ?? nil : nil
        return EmbeddedEventEnvelope(
            uri: (try? reader.stringField(table: table, index: 1)) ?? "",
            primaryId: primaryId,
            depth: (try? reader.u8Field(table: table, index: 2)) ?? 0,
            maxDepth: (try? reader.u8Field(table: table, index: 3)) ?? 4,
            projection: projection,
            collapsed: (try? reader.boolField(table: table, index: 4)) ?? false,
            collapseReason: reason
        )
    }

    private static func decodeProjection(
        _ table: Int,
        reader: GalleryFlatBufferReader
    ) -> EmbedKindProjection? {
        let kind = (try? reader.u8Field(table: table, index: 0)) ?? 4
        switch kind {
        case 0:
            guard let payload = try? reader.tableField(table: table, index: 1),
                  let value = decodeShortNote(payload, reader: reader) else { return nil }
            return .shortNote(value)
        case 1:
            guard let payload = try? reader.tableField(table: table, index: 2),
                  let value = decodeArticle(payload, reader: reader) else { return nil }
            return .article(value)
        case 2:
            guard let payload = try? reader.tableField(table: table, index: 3),
                  let value = decodeHighlight(payload, reader: reader) else { return nil }
            return .highlight(value)
        case 3:
            guard let payload = try? reader.tableField(table: table, index: 4),
                  let value = decodeProfile(payload, reader: reader) else { return nil }
            return .profile(value)
        default:
            guard let payload = try? reader.tableField(table: table, index: 5),
                  let value = decodeUnknown(payload, reader: reader) else { return nil }
            return .unknown(value)
        }
    }

    private static func decodeShortNote(
        _ table: Int,
        reader: GalleryFlatBufferReader
    ) -> ShortNoteProjection? {
        guard let id = try? reader.stringField(table: table, index: 0),
              let author = try? reader.stringField(table: table, index: 1) else {
            return nil
        }
        return ShortNoteProjection(
            id: id,
            authorPubkey: author,
            authorDisplayName: optionalString(table, 2, 3, reader),
            authorPictureUrl: optionalString(table, 4, 5, reader),
            createdAt: (try? reader.u64Field(table: table, index: 6)) ?? 0,
            content: contentText(table: table, index: 7, reader: reader),
            mediaUrls: (try? reader.stringVectorField(table: table, index: 8)) ?? []
        )
    }

    private static func decodeArticle(
        _ table: Int,
        reader: GalleryFlatBufferReader
    ) -> ArticleProjection? {
        guard let id = try? reader.stringField(table: table, index: 0),
              let author = try? reader.stringField(table: table, index: 1) else {
            return nil
        }
        return ArticleProjection(
            id: id,
            authorPubkey: author,
            authorDisplayName: optionalString(table, 2, 3, reader),
            authorPictureUrl: optionalString(table, 4, 5, reader),
            createdAt: (try? reader.u64Field(table: table, index: 6)) ?? 0,
            title: optionalString(table, 7, 8, reader),
            summary: optionalString(table, 9, 10, reader),
            heroImageUrl: optionalString(table, 11, 12, reader),
            dTag: (try? reader.stringField(table: table, index: 13)) ?? "",
            content: contentText(table: table, index: 14, reader: reader)
        )
    }

    private static func decodeHighlight(
        _ table: Int,
        reader: GalleryFlatBufferReader
    ) -> HighlightProjection? {
        guard let id = try? reader.stringField(table: table, index: 0),
              let author = try? reader.stringField(table: table, index: 1) else {
            return nil
        }
        return HighlightProjection(
            id: id,
            authorPubkey: author,
            authorDisplayName: optionalString(table, 2, 3, reader),
            createdAt: (try? reader.u64Field(table: table, index: 4)) ?? 0,
            highlightedText: (try? reader.stringField(table: table, index: 5)) ?? "",
            sourceEventId: optionalString(table, 6, 7, reader),
            sourceEventAddr: optionalString(table, 8, 9, reader),
            sourceUrl: optionalString(table, 10, 11, reader),
            context: optionalString(table, 12, 13, reader)
        )
    }

    private static func decodeProfile(
        _ table: Int,
        reader: GalleryFlatBufferReader
    ) -> ProfileProjection? {
        guard let pubkey = try? reader.stringField(table: table, index: 0) else { return nil }
        return ProfileProjection(
            pubkey: pubkey,
            displayName: optionalString(table, 1, 2, reader),
            pictureUrl: optionalString(table, 3, 4, reader),
            about: optionalString(table, 5, 6, reader),
            nip05: optionalString(table, 7, 8, reader),
            lud16: optionalString(table, 9, 10, reader),
            bannerUrl: optionalString(table, 11, 12, reader)
        )
    }

    private static func decodeUnknown(
        _ table: Int,
        reader: GalleryFlatBufferReader
    ) -> UnknownProjection? {
        guard let author = try? reader.stringField(table: table, index: 1) else { return nil }
        return UnknownProjection(
            kind: (try? reader.u32Field(table: table, index: 0)) ?? 0,
            authorPubkey: author,
            authorDisplayName: optionalString(table, 2, 3, reader),
            authorPictureUrl: optionalString(table, 4, 5, reader),
            createdAt: (try? reader.u64Field(table: table, index: 6)) ?? 0,
            content: (try? reader.stringField(table: table, index: 7)) ?? "",
            tags: decodeTags(table, reader: reader),
            altText: optionalString(table, 10, 11, reader)
        )
    }

    private static func decodeTags(_ table: Int, reader: GalleryFlatBufferReader) -> [[String]] {
        let rows = (try? reader.tableVectorField(table: table, index: 9)) ?? []
        return rows.map { row in
            (try? reader.stringVectorField(table: row, index: 0)) ?? []
        }
    }

    private static func contentText(
        table: Int,
        index: Int,
        reader: GalleryFlatBufferReader
    ) -> String {
        let data = (try? reader.bytesVectorField(table: table, index: index)) ?? Data()
        return GalleryContentTreePlainText.decode(data)
    }

    private static func optionalString(
        _ table: Int,
        _ presentIndex: Int,
        _ valueIndex: Int,
        _ reader: GalleryFlatBufferReader
    ) -> String? {
        guard ((try? reader.boolField(table: table, index: presentIndex)) ?? false) == true,
              let value = ((try? reader.stringField(table: table, index: valueIndex)) ?? nil),
              let string = value.galleryNonEmpty else {
            return nil
        }
        return string
    }
}
