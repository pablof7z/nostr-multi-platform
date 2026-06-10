import FlatBuffers
import Foundation

/// HAND-WRITTEN glue between the `flatc --swift` FlatBuffers reader structs and
/// the Chirp domain types, for the typed-projection-sidecar decode path.
///
/// ## Why this is hand-written, not generated
///
/// The generated `TypedProjectionDecoders.generated.swift` owns the mechanical
/// half of every typed-sidecar decoder: the `key`+`schemaId` envelope lookup
/// and the `getCheckedRoot(fileId:)` decode into the reader struct. The reader
/// struct's field layout (the FlatBuffer *wire*) does NOT field-align with the
/// Chirp *domain* type — the domain types are field-subsets of the wire, carry
/// `has_*` companion-bool optionals, and (for thick keys) nested sub-buffers.
/// A generic that mapped wire→domain across all keys would be leaky, so that
/// mapping stays here, one static per projection key, matching the
/// `swift_field` the registry assigns.
///
/// Each function takes the generated reader struct and returns the SAME Chirp
/// domain value the generic JSON `payload` path yields for that key, so a
/// consumer can read typed-first and fall back to JSON identically. NOTE: no
/// read site consumes these yet — this is the consumer-side FOUNDATION; wiring
/// the read sites (e.g. `KernelModel`/`KernelBridge`) is the follow-up batch.
/// Raw protocol values only (D11 — no display helpers).
enum TypedProjectionGlue {
    // MARK: accounts → [AccountSummary]

    /// Map the typed `accounts` sidecar (`KACC` / `nmp_kernel_AccountsSnapshot`)
    /// to the `[AccountSummary]` the JSON `projections.accounts` path yields.
    ///
    /// Each `AccountSummaryRow` mirrors the JSON `AccountSummary` field-for-field;
    /// the two `has_*` companion bools (`has_display_name`, `has_picture_url`)
    /// reproduce the JSON `null` / omitted-key semantics (ADR-0032).
    static func accounts(_ reader: nmp_kernel_AccountsSnapshot) -> [AccountSummary] {
        reader.accounts.map { row in
            AccountSummary(
                displayName: row.hasDisplayName ? (row.displayName ?? "") : nil,
                id: row.id ?? "",
                isActive: row.isActive,
                npub: row.npub ?? "",
                pictureUrl: row.hasPictureUrl ? (row.pictureUrl ?? "") : nil,
                signerIsRemote: row.signerIsRemote,
                signerKind: row.signerKind ?? "",
                signerLabel: row.signerLabel ?? "",
                status: row.status ?? ""
            )
        }
    }

    // MARK: active_account → String?

    /// Map the typed `active_account` sidecar (`KACT` /
    /// `nmp_kernel_ActiveAccountSnapshot`) to the `String?` the JSON
    /// `projections.active_account` path yields — `nil` when no account is
    /// active (`has_active_account == false` mirrors JSON `null`).
    static func activeAccount(_ reader: nmp_kernel_ActiveAccountSnapshot) -> String? {
        reader.hasActiveAccount ? (reader.pubkey ?? "") : nil
    }
}
