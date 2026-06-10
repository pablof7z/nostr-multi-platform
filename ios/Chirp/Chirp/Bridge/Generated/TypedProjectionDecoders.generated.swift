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
