import Foundation

enum GalleryTypedSnapshotDecodeError: LocalizedError {
    case invalidFrame(String)
    case schemaVersion(UInt32)

    var errorDescription: String? {
        switch self {
        case .invalidFrame(let message):
            return message
        case .schemaVersion(let version):
            return "unsupported SnapshotFrame schema_version \(version)"
        }
    }
}

private struct GalleryTypedProjectionEnvelope {
    let key: String
    let schemaId: String
    let schemaVersion: UInt32
    let fileIdentifier: String
    let payload: Data
    let state: UInt8
}

enum GalleryTypedSnapshotDecoder {
    private static let schemaVersion: UInt32 = 1

    static func snapshot(
        from data: Data,
        npubFor: (String) -> String?
    ) throws -> GallerySnapshot {
        let reader = GalleryFlatBufferReader(data: data)
        guard reader.hasIdentifier("NMPU") else {
            throw GalleryTypedSnapshotDecodeError.invalidFrame("missing NMPU file identifier")
        }
        let root = try reader.rootTable()
        guard ((try reader.u8Field(table: root, index: 0)) ?? 0) == 0 else {
            throw GalleryTypedSnapshotDecodeError.invalidFrame("non-snapshot update frame")
        }
        guard let snapshot = try reader.tableField(table: root, index: 1) else {
            throw GalleryTypedSnapshotDecodeError.invalidFrame("missing SnapshotFrame")
        }
        let version = (try reader.u32Field(table: snapshot, index: 0)) ?? schemaVersion
        guard version == schemaVersion else {
            throw GalleryTypedSnapshotDecodeError.schemaVersion(version)
        }

        let projections = try typedProjections(reader: reader, snapshot: snapshot)
        let embeds = projectionPayload(
            projections,
            key: "claimed_event_embeds",
            schemaId: "claimed_event_embeds",
            fileIdentifier: "NEMB"
        ).map(GalleryEmbedSidecarDecoder.decode)

        return GallerySnapshot(
            running: (try reader.boolField(table: snapshot, index: 7)) ?? false,
            profiles: decodeResolvedProfiles(projections, npubFor: npubFor),
            accounts: decodeAccounts(projections),
            claimedEventEmbeds: embeds,
            relayRoleOptions: decodeRelayRoleOptions(projections)
        )
    }

    private static func typedProjections(
        reader: GalleryFlatBufferReader,
        snapshot: Int
    ) throws -> [GalleryTypedProjectionEnvelope] {
        let rows = try reader.tableVectorField(table: snapshot, index: 2)
        return try rows.compactMap { row in
            guard let key = try reader.stringField(table: row, index: 0) else { return nil }
            let state = (try reader.u8Field(table: row, index: 3)) ?? 0
            if state == 1 {
                return GalleryTypedProjectionEnvelope(
                    key: key,
                    schemaId: "",
                    schemaVersion: 0,
                    fileIdentifier: "",
                    payload: Data(),
                    state: state
                )
            }
            guard let payloadTable = try reader.tableField(table: row, index: 1),
                  let schemaId = try reader.stringField(table: payloadTable, index: 0),
                  let fileIdentifier = try reader.stringField(table: payloadTable, index: 2) else {
                return nil
            }
            return GalleryTypedProjectionEnvelope(
                key: key,
                schemaId: schemaId,
                schemaVersion: (try reader.u32Field(table: payloadTable, index: 1)) ?? schemaVersion,
                fileIdentifier: fileIdentifier,
                payload: try reader.bytesVectorField(table: payloadTable, index: 3),
                state: state
            )
        }
    }

    private static func projectionPayload(
        _ projections: [GalleryTypedProjectionEnvelope],
        key: String,
        schemaId: String,
        fileIdentifier: String
    ) -> Data? {
        projections.first {
            $0.key == key
                && $0.schemaId == schemaId
                && $0.schemaVersion == schemaVersion
                && $0.fileIdentifier == fileIdentifier
                && $0.state != 1
                && !$0.payload.isEmpty
        }?.payload
    }

    private static func decodeResolvedProfiles(
        _ projections: [GalleryTypedProjectionEnvelope],
        npubFor: (String) -> String?
    ) -> [String: ProfileWire] {
        guard let payload = projectionPayload(
            projections,
            key: "resolved_profiles",
            schemaId: "resolved_profiles",
            fileIdentifier: "KRPR"
        ) else {
            return [:]
        }
        let reader = GalleryFlatBufferReader(data: payload)
        guard reader.hasIdentifier("KRPR"),
              let root = try? reader.rootTable(),
              let entries = try? reader.tableVectorField(table: root, index: 0) else {
            return [:]
        }
        var result: [String: ProfileWire] = [:]
        for entry in entries {
            guard let key = try? reader.stringField(table: entry, index: 0),
                  let card = try? reader.tableField(table: entry, index: 1),
                  let wire = profileWire(card: card, fallbackPubkey: key, reader: reader, npubFor: npubFor) else {
                continue
            }
            result[wire.pubkey] = wire
        }
        return result
    }

    private static func profileWire(
        card: Int,
        fallbackPubkey: String,
        reader: GalleryFlatBufferReader,
        npubFor: (String) -> String?
    ) -> ProfileWire? {
        let pubkey = ((try? reader.stringField(table: card, index: 0)) ?? nil) ?? fallbackPubkey
        let npub = npubFor(pubkey) ?? ""
        return ProfileWire(
            pubkey: pubkey,
            displayName: optionalString(card, 2, 3, reader),
            about: ((try? reader.stringField(table: card, index: 7)) ?? nil)?.galleryNonEmpty,
            pictureUrl: optionalString(card, 4, 5, reader),
            nip05: ((try? reader.stringField(table: card, index: 6)) ?? nil)?.galleryNonEmpty,
            npub: npub,
            npubShort: shortenIdentifier(npub.isEmpty ? pubkey : npub)
        )
    }

    private static func decodeAccounts(_ projections: [GalleryTypedProjectionEnvelope]) -> [AccountWire] {
        guard let payload = projectionPayload(
            projections,
            key: "accounts",
            schemaId: "accounts",
            fileIdentifier: "KACC"
        ) else {
            return []
        }
        let reader = GalleryFlatBufferReader(data: payload)
        guard reader.hasIdentifier("KACC"),
              let root = try? reader.rootTable(),
              let accounts = try? reader.tableVectorField(table: root, index: 0) else {
            return []
        }
        return accounts.compactMap { account in
            guard let id = try? reader.stringField(table: account, index: 0) else { return nil }
            return AccountWire(
                pubkey: id,
                active: (try? reader.boolField(table: account, index: 8)) ?? false
            )
        }
    }

    private static func decodeRelayRoleOptions(
        _ projections: [GalleryTypedProjectionEnvelope]
    ) -> [GalleryRelayRoleOption] {
        guard let payload = projectionPayload(
            projections,
            key: "relay_role_options",
            schemaId: "relay_role_options",
            fileIdentifier: "KRRO"
        ) else {
            return []
        }
        let reader = GalleryFlatBufferReader(data: payload)
        guard reader.hasIdentifier("KRRO"),
              let root = try? reader.rootTable(),
              let options = try? reader.tableVectorField(table: root, index: 0) else {
            return []
        }
        return options.map { option in
            GalleryRelayRoleOption(
                value: ((try? reader.stringField(table: option, index: 0)) ?? nil) ?? "",
                label: ((try? reader.stringField(table: option, index: 1)) ?? nil) ?? "",
                tint: ((try? reader.stringField(table: option, index: 2)) ?? nil) ?? "",
                isDefault: (try? reader.boolField(table: option, index: 3)) ?? false
            )
        }
    }

    private static func optionalString(
        _ table: Int,
        _ presentIndex: Int,
        _ valueIndex: Int,
        _ reader: GalleryFlatBufferReader
    ) -> String? {
        guard ((try? reader.boolField(table: table, index: presentIndex)) ?? false) == true else {
            return nil
        }
        return ((try? reader.stringField(table: table, index: valueIndex)) ?? nil)?.galleryNonEmpty
    }

    private static func shortenIdentifier(_ value: String) -> String {
        guard value.count > 12 else { return value }
        return "\(value.prefix(9))...\(value.suffix(4))"
    }
}

extension String {
    var galleryNonEmpty: String? { isEmpty ? nil : self }
}
