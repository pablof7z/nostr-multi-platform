// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen typed-decoders \
//       --out ios/Chirp/Chirp/Bridge/Generated/TypedProjectionDecoders.generated.swift
//
// Source of truth: the typed-sidecar identities in
// `crates/nmp-codegen/src/swift_projections_registry.rs`
// (`SnapshotProjectionEntry::typed_sidecar`). The CI gate
// (`.github/workflows/codegen-drift.yml`) fails any PR whose generated Swift
// differs from a fresh run.
//
// V6 Stage 4 (consumer-side). Each enum below is the GENERATED mechanical half
// of one projection's typed-sidecar decoder: the `key`+`schemaId` lookup over
// `[TypedProjectionEnvelope]` and the `getCheckedRoot(fileId:)` decode into the
// `flatc --swift` reader struct. The reader→Chirp-domain mapping is the
// HAND-WRITTEN `TypedProjectionGlue` seam (see
// `ios/Chirp/Chirp/Bridge/TypedProjectionGlue.swift`).
//
// Only projection keys whose `flatc --swift` reader binding is checked into the
// Chirp target appear here. The rest need their binding generated first.
// ─────────────────────────────────────────────────────────────────────────────

import FlatBuffers
import Foundation

// MARK: - TypedPublishQueueDecoder
// Projection `publish_queue` → typed sidecar `publish_queue` (KPBQ). Domain type: `[PublishQueueEntry]?`.
enum TypedPublishQueueDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "publish_queue"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "publish_queue"
    /// FlatBuffers `file_identifier` for `nmp_kernel_PublishQueueSnapshot`.
    static let fileIdentifier = "KPBQ"

    /// Decode the typed `publish_queue` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` (so the host
    /// falls back to the generic JSON `payload`) when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> [PublishQueueEntry]? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KPBQ` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> [PublishQueueEntry]? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        guard let reader: nmp_kernel_PublishQueueSnapshot = try? getCheckedRoot(
            byteBuffer: &buffer,
            fileId: fileIdentifier
        ) else {
            return nil
        }
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.publishQueue`.
        return TypedProjectionGlue.publishQueue(reader)
    }
}

// MARK: - TypedPublishOutboxDecoder
// Projection `publish_outbox` → typed sidecar `publish_outbox` (KPBO). Domain type: `[PublishOutboxItem]?`.
enum TypedPublishOutboxDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "publish_outbox"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "publish_outbox"
    /// FlatBuffers `file_identifier` for `nmp_kernel_PublishOutboxSnapshot`.
    static let fileIdentifier = "KPBO"

    /// Decode the typed `publish_outbox` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` (so the host
    /// falls back to the generic JSON `payload`) when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> [PublishOutboxItem]? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KPBO` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> [PublishOutboxItem]? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        guard let reader: nmp_kernel_PublishOutboxSnapshot = try? getCheckedRoot(
            byteBuffer: &buffer,
            fileId: fileIdentifier
        ) else {
            return nil
        }
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.publishOutbox`.
        return TypedProjectionGlue.publishOutbox(reader)
    }
}

// MARK: - TypedOutboxSummaryDecoder
// Projection `outbox_summary` → typed sidecar `outbox_summary` (KOXS). Domain type: `OutboxSummary?`.
enum TypedOutboxSummaryDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "outbox_summary"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "outbox_summary"
    /// FlatBuffers `file_identifier` for `nmp_kernel_OutboxSummarySnapshot`.
    static let fileIdentifier = "KOXS"

    /// Decode the typed `outbox_summary` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` (so the host
    /// falls back to the generic JSON `payload`) when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> OutboxSummary? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KOXS` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> OutboxSummary? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        guard let reader: nmp_kernel_OutboxSummarySnapshot = try? getCheckedRoot(
            byteBuffer: &buffer,
            fileId: fileIdentifier
        ) else {
            return nil
        }
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.outboxSummary`.
        return TypedProjectionGlue.outboxSummary(reader)
    }
}

// MARK: - TypedConfiguredRelaysDecoder
// Projection `configured_relays` → typed sidecar `configured_relays` (KCRL). Domain type: `[AppRelay]?`.
enum TypedConfiguredRelaysDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "configured_relays"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "configured_relays"
    /// FlatBuffers `file_identifier` for `nmp_kernel_ConfiguredRelaysSnapshot`.
    static let fileIdentifier = "KCRL"

    /// Decode the typed `configured_relays` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` (so the host
    /// falls back to the generic JSON `payload`) when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> [AppRelay]? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KCRL` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> [AppRelay]? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        guard let reader: nmp_kernel_ConfiguredRelaysSnapshot = try? getCheckedRoot(
            byteBuffer: &buffer,
            fileId: fileIdentifier
        ) else {
            return nil
        }
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.configuredRelays`.
        return TypedProjectionGlue.configuredRelays(reader)
    }
}

// MARK: - TypedRelayRoleOptionsDecoder
// Projection `relay_role_options` → typed sidecar `relay_role_options` (KRRO). Domain type: `[RelayRoleOption]?`.
enum TypedRelayRoleOptionsDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "relay_role_options"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "relay_role_options"
    /// FlatBuffers `file_identifier` for `nmp_kernel_RelayRoleOptionsSnapshot`.
    static let fileIdentifier = "KRRO"

    /// Decode the typed `relay_role_options` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` (so the host
    /// falls back to the generic JSON `payload`) when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> [RelayRoleOption]? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KRRO` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> [RelayRoleOption]? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        guard let reader: nmp_kernel_RelayRoleOptionsSnapshot = try? getCheckedRoot(
            byteBuffer: &buffer,
            fileId: fileIdentifier
        ) else {
            return nil
        }
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.relayRoleOptions`.
        return TypedProjectionGlue.relayRoleOptions(reader)
    }
}

// MARK: - TypedAccountsDecoder
// Projection `accounts` → typed sidecar `accounts` (KACC). Domain type: `[AccountSummary]?`.
enum TypedAccountsDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "accounts"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "accounts"
    /// FlatBuffers `file_identifier` for `nmp_kernel_AccountsSnapshot`.
    static let fileIdentifier = "KACC"

    /// Decode the typed `accounts` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` (so the host
    /// falls back to the generic JSON `payload`) when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> [AccountSummary]? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KACC` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> [AccountSummary]? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        guard let reader: nmp_kernel_AccountsSnapshot = try? getCheckedRoot(
            byteBuffer: &buffer,
            fileId: fileIdentifier
        ) else {
            return nil
        }
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.accounts`.
        return TypedProjectionGlue.accounts(reader)
    }
}

// MARK: - TypedActiveAccountDecoder
// Projection `active_account` → typed sidecar `active_account` (KACT). Domain type: `String?`.
enum TypedActiveAccountDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "active_account"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "active_account"
    /// FlatBuffers `file_identifier` for `nmp_kernel_ActiveAccountSnapshot`.
    static let fileIdentifier = "KACT"

    /// Decode the typed `active_account` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` (so the host
    /// falls back to the generic JSON `payload`) when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> String? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KACT` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> String? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        guard let reader: nmp_kernel_ActiveAccountSnapshot = try? getCheckedRoot(
            byteBuffer: &buffer,
            fileId: fileIdentifier
        ) else {
            return nil
        }
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.activeAccount`.
        return TypedProjectionGlue.activeAccount(reader)
    }
}
