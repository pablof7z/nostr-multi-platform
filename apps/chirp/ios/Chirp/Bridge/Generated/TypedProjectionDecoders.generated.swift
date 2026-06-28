// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen typed-decoders \
//       --out apps/chirp/ios/Chirp/Bridge/Generated/TypedProjectionDecoders.generated.swift
//
// Source of truth: the typed-sidecar identities in
// `crates/nmp-codegen/src/swift_projections_registry.rs`
// (`SnapshotProjectionEntry::typed_sidecar`). The CI gate
// (`.github/workflows/codegen-drift.yml`) fails any PR whose generated Swift
// differs from a fresh run.
//
// V6 Stage 4 (consumer-side). Each enum below is the GENERATED mechanical half
// of one projection's typed-sidecar decoder: the `key`+`schemaId` lookup over
// `[TypedProjectionEnvelope]` and the `getRoot(byteBuffer:)` (unchecked) decode
// into the `flatc --swift` reader struct. Buffers arrive over a trusted
// in-process FFI boundary (Rust kernel → Swift shell, same process/memory);
// running the O(buffer) FlatBuffers Verifier on the 4 Hz hot path is pure waste.
// The reader→Chirp-domain mapping is the HAND-WRITTEN `TypedProjectionGlue` seam
// (see `apps/chirp/ios/Chirp/Bridge/TypedProjectionGlue.swift`).
//
// Only projection keys whose `flatc --swift` reader binding is checked into the
// Chirp target appear here. The rest need their binding generated first.
// ─────────────────────────────────────────────────────────────────────────────

import FlatBuffers
import Foundation

// MARK: - TypedWalletDecoder
// Projection `wallet` → typed sidecar `nmp.nip47.wallet` (NWST). Domain type: `WalletStatusData?`.
enum TypedWalletDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "wallet"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "nmp.nip47.wallet"
    /// FlatBuffers `file_identifier` for `nmp_nip47_WalletStatus`.
    static let fileIdentifier = "NWST"

    /// Decode the typed `wallet` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> WalletStatusData? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NWST` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> WalletStatusData? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_nip47_WalletStatus = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.wallet`.
        return TypedProjectionGlue.wallet(reader)
    }
}

// MARK: - TypedBunkerHandshakeDecoder
// Projection `bunker_handshake` → typed sidecar `bunker_handshake` (KBHS). Domain type: `BunkerHandshake?`.
enum TypedBunkerHandshakeDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "bunker_handshake"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "bunker_handshake"
    /// FlatBuffers `file_identifier` for `nmp_kernel_BunkerHandshake`.
    static let fileIdentifier = "KBHS"

    /// Decode the typed `bunker_handshake` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> BunkerHandshake? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KBHS` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> BunkerHandshake? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_BunkerHandshake = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.bunkerHandshake`.
        return TypedProjectionGlue.bunkerHandshake(reader)
    }
}

// MARK: - TypedNip46OnboardingDecoder
// Projection `nip46_onboarding` → typed sidecar `nip46_onboarding` (KN46). Domain type: `Nip46Onboarding?`.
enum TypedNip46OnboardingDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "nip46_onboarding"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "nip46_onboarding"
    /// FlatBuffers `file_identifier` for `nmp_kernel_Nip46Onboarding`.
    static let fileIdentifier = "KN46"

    /// Decode the typed `nip46_onboarding` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> Nip46Onboarding? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KN46` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> Nip46Onboarding? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_Nip46Onboarding = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.nip46Onboarding`.
        return TypedProjectionGlue.nip46Onboarding(reader)
    }
}

// MARK: - TypedSignerStateDecoder
// Projection `signer_state` → typed sidecar `signer_state` (KSST). Domain type: `SignerState?`.
enum TypedSignerStateDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "signer_state"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "signer_state"
    /// FlatBuffers `file_identifier` for `nmp_kernel_SignerState`.
    static let fileIdentifier = "KSST"

    /// Decode the typed `signer_state` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> SignerState? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KSST` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> SignerState? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_SignerState = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.signerState`.
        return TypedProjectionGlue.signerState(reader)
    }
}

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
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
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
        let reader: nmp_kernel_PublishQueueSnapshot = getRoot(byteBuffer: &buffer)
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
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
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
        let reader: nmp_kernel_PublishOutboxSnapshot = getRoot(byteBuffer: &buffer)
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
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
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
        let reader: nmp_kernel_OutboxSummarySnapshot = getRoot(byteBuffer: &buffer)
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
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
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
        let reader: nmp_kernel_ConfiguredRelaysSnapshot = getRoot(byteBuffer: &buffer)
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
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
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
        let reader: nmp_kernel_RelayRoleOptionsSnapshot = getRoot(byteBuffer: &buffer)
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
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
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
        let reader: nmp_kernel_AccountsSnapshot = getRoot(byteBuffer: &buffer)
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
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
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
        let reader: nmp_kernel_ActiveAccountSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.activeAccount`.
        return TypedProjectionGlue.activeAccount(reader)
    }
}

// MARK: - TypedActionResultsDecoder
// Projection `action_results` → typed sidecar `action_results` (KARS). Domain type: `[LastActionResult]?`.
enum TypedActionResultsDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "action_results"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "action_results"
    /// FlatBuffers `file_identifier` for `nmp_kernel_ActionResultsSnapshot`.
    static let fileIdentifier = "KARS"

    /// Decode the typed `action_results` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> [LastActionResult]? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KARS` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> [LastActionResult]? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_ActionResultsSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.actionResults`.
        return TypedProjectionGlue.actionResults(reader)
    }
}

// MARK: - TypedActionStagesDecoder
// Projection `action_stages` → typed sidecar `action_stages` (KAST). Domain type: `[String: [ActionStageEntry]]?`.
enum TypedActionStagesDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "action_stages"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "action_stages"
    /// FlatBuffers `file_identifier` for `nmp_kernel_ActionStagesSnapshot`.
    static let fileIdentifier = "KAST"

    /// Decode the typed `action_stages` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> [String: [ActionStageEntry]]? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KAST` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> [String: [ActionStageEntry]]? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_ActionStagesSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.actionStages`.
        return TypedProjectionGlue.actionStages(reader)
    }
}

// MARK: - TypedActionLifecycleDecoder
// Projection `action_lifecycle` → typed sidecar `action_lifecycle` (KALC). Domain type: `ActionLifecycleSnapshot?`.
enum TypedActionLifecycleDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "action_lifecycle"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "action_lifecycle"
    /// FlatBuffers `file_identifier` for `nmp_kernel_ActionLifecycleSnapshot`.
    static let fileIdentifier = "KALC"

    /// Decode the typed `action_lifecycle` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> ActionLifecycleSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KALC` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> ActionLifecycleSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_ActionLifecycleSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.actionLifecycle`.
        return TypedProjectionGlue.actionLifecycle(reader)
    }
}

// MARK: - TypedProfileDecoder
// Projection `profile` → typed sidecar `profile` (KPRF). Domain type: `ProfileCard?`.
enum TypedProfileDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "profile"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "profile"
    /// FlatBuffers `file_identifier` for `nmp_kernel_ProfileSnapshot`.
    static let fileIdentifier = "KPRF"

    /// Decode the typed `profile` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> ProfileCard? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KPRF` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> ProfileCard? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_ProfileSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.profile`.
        return TypedProjectionGlue.profile(reader)
    }
}

// MARK: - TypedGroupEventsDecoder
// Projection `nmp.nip29.group_events` → typed sidecar `nmp.nip29.group_events` (NGEV). Domain type: `GroupEventsSnapshot?`.
enum TypedGroupEventsDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "nmp.nip29.group_events"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "nmp.nip29.group_events"
    /// FlatBuffers `file_identifier` for `nmp_nip29_GroupEventsSnapshot`.
    static let fileIdentifier = "NGEV"

    /// Decode the typed `nmp.nip29.group_events` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> GroupEventsSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NGEV` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> GroupEventsSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_nip29_GroupEventsSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.groupEvents`.
        return TypedProjectionGlue.groupEvents(reader)
    }
}

// MARK: - TypedDmInboxDecoder
// Projection `nmp.nip17.dm_inbox` → typed sidecar `nmp.nip17.dm_inbox` (NDMI). Domain type: `DmInboxSnapshot?`.
enum TypedDmInboxDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "nmp.nip17.dm_inbox"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "nmp.nip17.dm_inbox"
    /// FlatBuffers `file_identifier` for `nmp_nip17_DmInboxSnapshot`.
    static let fileIdentifier = "NDMI"

    /// Decode the typed `nmp.nip17.dm_inbox` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> DmInboxSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NDMI` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> DmInboxSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_nip17_DmInboxSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.dmInbox`.
        return TypedProjectionGlue.dmInbox(reader)
    }
}

// MARK: - TypedFollowListDecoder
// Projection `nmp.follow_list` → typed sidecar `nmp.nip02.follow_list` (NF02). Domain type: `FollowListSnapshot?`.
enum TypedFollowListDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "nmp.follow_list"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "nmp.nip02.follow_list"
    /// FlatBuffers `file_identifier` for `nmp_nip02_FollowListSnapshot`.
    static let fileIdentifier = "NF02"

    /// Decode the typed `nmp.follow_list` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> FollowListSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NF02` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> FollowListSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_nip02_FollowListSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.followList`.
        return TypedProjectionGlue.followList(reader)
    }
}

// MARK: - TypedDiscoveredGroupsDecoder
// Projection `nmp.nip29.discovered_groups` → typed sidecar `nmp.nip29.discovered_groups` (NDGS). Domain type: `DiscoveredGroupsSnapshot?`.
enum TypedDiscoveredGroupsDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "nmp.nip29.discovered_groups"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "nmp.nip29.discovered_groups"
    /// FlatBuffers `file_identifier` for `nmp_nip29_DiscoveredGroupsSnapshot`.
    static let fileIdentifier = "NDGS"

    /// Decode the typed `nmp.nip29.discovered_groups` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> DiscoveredGroupsSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NDGS` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> DiscoveredGroupsSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_nip29_DiscoveredGroupsSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.discoveredGroups`.
        return TypedProjectionGlue.discoveredGroups(reader)
    }
}

// MARK: - TypedGroupDefaultsDecoder
// Projection `nmp.nip29.group_defaults` → typed sidecar `nmp.nip29.group_defaults` (NGDF). Domain type: `GroupDefaultsSnapshot?`.
enum TypedGroupDefaultsDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "nmp.nip29.group_defaults"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "nmp.nip29.group_defaults"
    /// FlatBuffers `file_identifier` for `nmp_nip29_GroupDefaultsSnapshot`.
    static let fileIdentifier = "NGDF"

    /// Decode the typed `nmp.nip29.group_defaults` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> GroupDefaultsSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NGDF` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> GroupDefaultsSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_nip29_GroupDefaultsSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.groupDefaults`.
        return TypedProjectionGlue.groupDefaults(reader)
    }
}

// MARK: - TypedDmRelayListDecoder
// Projection `nmp.nip17.dm_relay_list` → typed sidecar `nmp.nip17.dm_relay_list` (NDRL). Domain type: `DmRelayListSnapshot?`.
enum TypedDmRelayListDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "nmp.nip17.dm_relay_list"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "nmp.nip17.dm_relay_list"
    /// FlatBuffers `file_identifier` for `nmp_nip17_DmRelayListSnapshot`.
    static let fileIdentifier = "NDRL"

    /// Decode the typed `nmp.nip17.dm_relay_list` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> DmRelayListSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NDRL` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> DmRelayListSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_nip17_DmRelayListSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.dmRelayList`.
        return TypedProjectionGlue.dmRelayList(reader)
    }
}

// MARK: - TypedRelayDiagnosticsDecoder
// Projection `relay_diagnostics` → typed sidecar `relay_diagnostics` (KRDG). Domain type: `RelayDiagnosticsSnapshot?`.
enum TypedRelayDiagnosticsDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "relay_diagnostics"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "relay_diagnostics"
    /// FlatBuffers `file_identifier` for `nmp_kernel_RelayDiagnosticsSnapshot`.
    static let fileIdentifier = "KRDG"

    /// Decode the typed `relay_diagnostics` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> RelayDiagnosticsSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KRDG` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> RelayDiagnosticsSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_RelayDiagnosticsSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.relayDiagnostics`.
        return TypedProjectionGlue.relayDiagnostics(reader)
    }
}

// MARK: - TypedRefEventEnvelopesDecoder
// Projection `refs.event.envelopes` → typed sidecar `refs.event.envelopes` (NEMB). Domain type: `[String: EmbeddedEventEnvelope]?`.
enum TypedRefEventEnvelopesDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "refs.event.envelopes"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "refs.event.envelopes"
    /// FlatBuffers `file_identifier` for `nmp_embed_RefEventEnvelopes`.
    static let fileIdentifier = "NEMB"

    /// Decode the typed `refs.event.envelopes` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> [String: EmbeddedEventEnvelope]? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NEMB` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> [String: EmbeddedEventEnvelope]? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_embed_RefEventEnvelopes = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.refEventEnvelopes`.
        return TypedProjectionGlue.refEventEnvelopes(reader)
    }
}

// MARK: - TypedSettingsHubDecoder
// Projection `settings_hub` → typed sidecar `settings_hub` (KSHB). Domain type: `[String: Int]?`.
enum TypedSettingsHubDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "settings_hub"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "settings_hub"
    /// FlatBuffers `file_identifier` for `nmp_kernel_SettingsHubSnapshot`.
    static let fileIdentifier = "KSHB"

    /// Decode the typed `settings_hub` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> [String: Int]? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `KSHB` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> [String: Int]? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_SettingsHubSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.settingsHub`.
        return TypedProjectionGlue.settingsHub(reader)
    }
}

// MARK: - TypedMarmotSnapshotDecoder
// Projection `nmp.marmot.snapshot` → typed sidecar `nmp.marmot.snapshot` (NMMS). Domain type: `MarmotSnapshot?`.
enum TypedMarmotSnapshotDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "nmp.marmot.snapshot"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "nmp.marmot.snapshot"
    /// FlatBuffers `file_identifier` for `nmp_marmot_MarmotSnapshot`.
    static let fileIdentifier = "NMMS"

    /// Decode the typed `nmp.marmot.snapshot` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> MarmotSnapshot? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NMMS` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> MarmotSnapshot? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_marmot_MarmotSnapshot = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.marmotSnapshot`.
        return TypedProjectionGlue.marmotSnapshot(reader)
    }
}

// MARK: - TypedMarmotMessagesDecoder
// Projection `nmp.marmot.messages` → typed sidecar `nmp.marmot.messages` (NMMG). Domain type: `[String: [MarmotMessage]]?`.
enum TypedMarmotMessagesDecoder {
    /// `TypedProjection.key` the producer publishes for this projection.
    static let key = "nmp.marmot.messages"
    /// `TypedPayload.schema_id` carried on the sidecar buffer.
    static let schemaId = "nmp.marmot.messages"
    /// FlatBuffers `file_identifier` for `nmp_marmot_MarmotMessages`.
    static let fileIdentifier = "NMMG"

    /// Decode the typed `nmp.marmot.messages` sidecar from the snapshot's typed-projection
    /// envelopes into the Chirp domain value. Returns `nil` when the sidecar is absent,
    /// carries the wrong schema, or is not a well-formed buffer.
    static func decode(from projections: [TypedProjectionEnvelope]) -> [String: [MarmotMessage]]? {
        guard let projection = projections.first(where: {
            $0.key == key && $0.schemaId == schemaId
        }), !projection.payload.isEmpty else {
            return nil
        }
        return decode(bytes: projection.payload)
    }

    /// Decode a raw `NMMG` FlatBuffers buffer into the Chirp domain value.
    static func decode(bytes: Data) -> [String: [MarmotMessage]]? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_marmot_MarmotMessages = getRoot(byteBuffer: &buffer)
        // Hand-written glue (NOT generated): map the `flatc --swift` reader
        // struct to the Chirp domain type. See `TypedProjectionGlue.marmotMessages`.
        return TypedProjectionGlue.marmotMessages(reader)
    }
}
