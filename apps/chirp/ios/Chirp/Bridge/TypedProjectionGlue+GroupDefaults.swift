import FlatBuffers
import Foundation

/// `nmp.nip29.group_defaults → GroupDefaultsSnapshot` typed-sidecar glue (#626).
///
/// Extracted from `TypedProjectionGlue.swift` as a sibling extension to keep the
/// hand-written glue file under the file-size ceiling. The static remains on
/// `TypedProjectionGlue`, so the generated decoder's call
/// (`TypedProjectionGlue.groupDefaults(reader)` in
/// `TypedProjectionDecoders.generated.swift`) is unchanged.
extension TypedProjectionGlue {
    // MARK: nmp.nip29.group_defaults → GroupDefaultsSnapshot

    /// Map the typed `nmp.nip29.group_defaults` sidecar (`NGDF` /
    /// `nmp_nip29_GroupDefaultsSnapshot`) to the `GroupDefaultsSnapshot` the JSON
    /// `projections["nmp.nip29.group_defaults"]` path yields (#626). Flat
    /// single-field copy: `suggestedRelayUrl` is the app/operator-owned default
    /// host relay URL for a new public group, carried verbatim (raw protocol
    /// value; the shell pre-fills it but the user may overwrite it).
    static func groupDefaults(
        _ reader: nmp_nip29_GroupDefaultsSnapshot
    ) -> GroupDefaultsSnapshot {
        GroupDefaultsSnapshot(suggestedRelayUrl: reader.suggestedRelayUrl ?? "")
    }
}
