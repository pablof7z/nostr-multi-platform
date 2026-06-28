import FlatBuffers
import Foundation

/// HAND-WRITTEN refs-row glue for the ADR-0063 Lane C (#1671) keyed reference
/// cache. Split out of `TypedProjectionGlue.swift` (codex NIT) so that file
/// stays under its file-size baseline. Same `enum TypedProjectionGlue` namespace
/// via an extension — the generated `KeyedRefCache.generated.swift` typed decoder
/// calls `TypedProjectionGlue.refRowEvent(reader)` exactly as before.
extension TypedProjectionGlue {
    // MARK: refs.event row → ClaimedEventDto (ADR-0063 Lane C, #1671)

    /// Map ONE `refs.event` row payload buffer — a `KCEV`
    /// `nmp_kernel_ClaimedEventsSnapshot` carrying EXACTLY ONE entry — to that one
    /// `ClaimedEventDto`. The `KeyedRefCache.event(primaryId)` typed accessor
    /// calls this so a view binds one decoded event, not a dict.
    ///
    /// FAIL-CLOSED single-entry contract (codex BLOCKING): the kernel's
    /// `ref_event_row_payload` (`crates/nmp-core/src/kernel/ref_row_source.rs`)
    /// encodes a `ClaimedEventsModel { entries: vec![(key, …one row…)] }` — EXACTLY
    /// ONE entry per `refs.event` row, always. So a buffer with 0 OR 2+ entries is
    /// MALFORMED. Returning the first of several would let a corrupt multi-entry
    /// KCEV pass decode-before-commit and silently commit the wrong row. We require
    /// `reader.entries.count == 1` and return `nil` otherwise, so the
    /// decode-before-commit seam rejects the row (prior row retained, needsResync
    /// latched) rather than committing a forged event.
    static func refRowEvent(
        _ reader: nmp_kernel_ClaimedEventsSnapshot
    ) -> ClaimedEventDto? {
        // Exactly one entry, or fail closed. `entries.count` reads the flatbuffer
        // vector length without materializing it.
        guard reader.entries.count == 1 else { return nil }
        guard let entry = reader.entries.first, let event = entry.value else { return nil }
        return ClaimedEventDto(
            id: event.id ?? "",
            authorPubkey: event.authorPubkey ?? "",
            kind: Int(event.kind),
            createdAt: Int(event.createdAt),
            content: event.content ?? "",
            tags: event.tags.map { row in row.values.map { $0 ?? "" } },
            signedEventJson: event.hasSignedEventJson ? (event.signedEventJson ?? "") : nil
        )
    }
}
