// GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --registry <app>/action-builders.json \
//       --platform swift --out <output>
//
// Source of truth: app-local action-builders registry JSON passed via
// `--registry`. NOT NMP's built-in `ACTION_BUILDERS` table.

import FlatBuffers
import Foundation

public enum GeneratedActionBuilders {
    public enum PublishSignerProvenance: String {
        case appManaged = "app_managed"
        case userSelected = "user_selected"
        case protocolPinned = "protocol_pinned"
        case diagnostic = "diagnostic"
    }

    public enum PublishSignerSelection {
        case active
        case registered(pubkey: String, provenance: PublishSignerProvenance)
    }

    public enum PublishRouteClass: String {
        case manualOverride = "manual_override"
        case groupHostPin = "group_host_pin"
        case verifiedPrivateInbox = "verified_private_inbox"
        case importedOrPresigned = "imported_or_presigned"
        case diagnostic = "diagnostic"
    }

    public enum PublishTargetSelection {
        case auto
        case explicit(relays: [String], routeClass: PublishRouteClass)
    }

    /// The single recognised envelope schema version — mirrors
    /// `nmp_core::dispatch_envelope::DISPATCH_ENVELOPE_SCHEMA_VERSION`.
    public static let dispatchEnvelopeSchemaVersion: UInt32 = 1

    /// Stamp `(correlationId, actionNamespace, schemaVersion, payload)` into a
    /// `DispatchEnvelope` and return the finished bytes (file identifier `NMPD`).
    /// The byte-for-byte twin of `encode_dispatch_envelope` in `nmp-core`.
    private static func encodeDispatchEnvelope(
        correlationId: String,
        actionNamespace: String,
        payload: [UInt8]
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let correlationOffset = fbb.create(string: correlationId)
        let namespaceOffset = fbb.create(string: actionNamespace)
        let payloadOffset = fbb.createVector(payload)
        let start = fbb.startTable(with: 4)
        fbb.add(offset: correlationOffset, at: 4)   // slot 0: correlation_id
        fbb.add(offset: namespaceOffset, at: 6)     // slot 1: action_namespace
        fbb.add(element: dispatchEnvelopeSchemaVersion, def: UInt32(0), at: 8) // slot 2: schema_version
        fbb.add(offset: payloadOffset, at: 10)      // slot 3: payload
        let root = Offset(offset: fbb.endTable(at: start))
        fbb.finish(offset: root, fileId: "NMPD")
        return fbb.sizedByteArray
    }

    /// Map a relay role string to the RelayMarker ubyte (Both=0, Read=1, Write=2, Indexer=3),
    /// mirroring `RelayMarker::from_role_string` in `nmp-router` EXACTLY — including rejection.
    /// Unknown tokens or no-flag input (e.g. empty string) encode as 255 (out-of-range sentinel)
    /// so the Rust decoder (`marker_from_wire`) fails closed instead of silently becoming Both.
    /// Role strings may be comma-separated (e.g. `"both,indexer"`); comparisons are case-insensitive.
    private static func relayMarkerByte(_ role: String) -> UInt8 {
        var hasBoth = false; var hasRead = false; var hasWrite = false; var hasIndexer = false
        var invalid = false
        for part in role.split(separator: ",").map({ $0.trimmingCharacters(in: .whitespaces).lowercased() }) {
            switch part {
            case "": break
            case "both": hasBoth = true
            case "read": hasRead = true
            case "write": hasWrite = true
            case "indexer": hasIndexer = true
            default: invalid = true
            }
        }
        if invalid { return 255 }
        if hasBoth || (hasRead && hasWrite) { return 0 }
        if hasRead { return 1 }
        if hasWrite { return 2 }
        if hasIndexer { return 3 }
        return 255
    }

    /// Publish the starter app's private status event.
    /// Builds the `app.login_timeline.publish_status` `DispatchEnvelope` bytes for the byte doorway.
    public static func publishStatus(
        correlationId: String,
        title: String,
        body: String,
        topics: [String]?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let titleOffset = fbb.create(string: title)
        let bodyOffset = fbb.create(string: body)
        let topicsOffset: Offset = {
            guard let values = topics, !values.isEmpty else { return Offset() }
            let offsets = values.map { fbb.create(string: $0) }
            return fbb.createVector(ofOffsets: offsets)
        }()
        let payloadStart = fbb.startTable(with: 4)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: titleOffset, at: 6) // slot 1: title
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        if topicsOffset.o != 0 { fbb.add(offset: topicsOffset, at: 10) } // slot 3: topics
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "APPS")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "app.login_timeline.publish_status",
            payload: payload
        )
    }
}
