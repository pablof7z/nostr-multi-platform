//! ADR-0063 (#1671) — the KEYED reference projection registry
//! (`KeyedProjectionEntry` + `KEYED_PROJECTIONS`) and its Lane-C typed
//! ROW-PAYLOAD descriptors (`RefRowPayload` / `KotlinRefRowPayload`), split out
//! of `swift_projections_registry.rs` to keep that file under the 500-LOC cap.
//! Re-exported through `swift_projections_registry` so the keyed-cache
//! generators' import paths are unchanged.

/// ADR-0063 Lane C (#1671) — the typed row-payload descriptor for one keyed
/// namespace. Identifies the `flatc` reader struct + `file_identifier` the host
/// decodes each `Changed` row payload into, the domain type the typed accessor
/// returns, and the hand-written `TypedProjectionGlue` entry that maps the
/// reader → domain. Reuses an EXISTING projection schema (the row payload IS a
/// `ProfileSnapshot` / `ClaimedEventsSnapshot` buffer), so all three already
/// ship; the generator just emits the per-row decode wiring.
pub struct RefRowPayload {
    /// FlatBuffers `file_identifier` of the ROW payload buffer (NOT the `NRRD`
    /// batch — that is [`KeyedProjectionEntry::file_identifier`]). e.g. `"KPRF"`
    /// for `ProfileSnapshot`, `"KCEV"` for `ClaimedEventsSnapshot`.
    pub row_file_identifier: &'static str,
    /// The `flatc --swift` reader struct for the row payload buffer
    /// (`nmp_kernel_ProfileSnapshot` / `nmp_kernel_ClaimedEventsSnapshot`).
    pub swift_reader_type: &'static str,
    /// The Swift domain type the typed accessor returns (without trailing `?`).
    /// `"ProfileCard"` for `refs.profile`; `"ClaimedEventDto"` for `refs.event`
    /// (the single-entry event row is unwrapped to one `ClaimedEventDto`).
    pub swift_domain_type: &'static str,
    /// The hand-written `TypedProjectionGlue` static (in
    /// `ios/Chirp/Chirp/Bridge/TypedProjectionGlue.swift`) that maps the reader
    /// struct to the domain value. For `refs.profile` this is the existing
    /// `profile(_:)` glue (reader is the SAME `ProfileSnapshot`); for
    /// `refs.event` it is the Lane-C `refRowEvent(_:)` glue that unwraps the
    /// single `ClaimedEventEntry`.
    pub swift_glue: &'static str,
    /// The Kotlin row-payload typed-decode descriptor, or `None` when the
    /// `flatc --kotlin` reader for the row payload buffer
    /// (`nmp.kernel.ProfileSnapshot` / `nmp.kernel.ClaimedEventsSnapshot`) is NOT
    /// yet checked into the Android target.
    ///
    /// This mirrors the [`crate::swift_typed_decoders`] precedent: a generated
    /// typed accessor references the reader class BY NAME, so it can only be
    /// emitted once that class ships. The KPRF `ProfileSnapshot` and KCEV
    /// `ClaimedEventsSnapshot` Kotlin readers are NOT checked in today (only the
    /// inner `ProfileCard.kt` is), so both entries carry `None` and the Kotlin
    /// generator falls back to the Lane-A raw `ByteArray?` accessor for them. The
    /// named follow-up is the `flatc --kotlin` binding (the schemas already
    /// declare `namespace nmp.kernel`, so a fresh `ci/regenerate-flatbuffers.sh`
    /// run emits them) + a drift-gate root-list addition — at which point this
    /// flips to `Some` and the Kotlin accessor becomes typed too, with zero
    /// generator change.
    pub kotlin: Option<KotlinRefRowPayload>,
}

/// The Kotlin-side typed row-payload descriptor (present only once the
/// `flatc --kotlin` reader for the row buffer ships). See
/// [`RefRowPayload::kotlin`].
pub struct KotlinRefRowPayload {
    /// The Kotlin reader class for the row payload buffer
    /// (`nmp.kernel.ProfileSnapshot` / `nmp.kernel.ClaimedEventsSnapshot`).
    pub reader_type: &'static str,
    /// The Kotlin domain type the typed accessor returns.
    pub domain_type: &'static str,
    /// The hand-written Kotlin glue function (in `KeyedRefDecoders.kt`) that maps
    /// the reader → domain value.
    pub glue: &'static str,
}

/// ADR-0063 Lane A (#1671) — one KEYED (row-grain) reference projection.
///
/// Keyed projections (`refs.profile` / `refs.event`) differ from the whole-value
/// `SnapshotProjectionEntry` above: their `TypedPayload.payload` is a
/// `nmp.refs.RefRowDeltaBatch` (a per-key row delta), and the host caches them as
/// `key -> rowPayload` with per-key observable slots, not as one value. They live
/// in a DEDICATED registry (not a `keyed: bool` flag) so the whole-value
/// generators (JSON struct/`CodingKeys`, typed decoders, projection cache) keep a
/// single-value-per-key contract; the keyed-cache generators consume THIS list.
pub struct KeyedProjectionEntry {
    /// Kernel-emitted projection key, e.g. `"refs.profile"`. The host routes a
    /// frame's `TypedProjection.key` to the keyed cache by matching this.
    pub projection_key: &'static str,
    /// The resolver namespace inside the `RefRowDeltaBatch`, e.g. `"profile"`.
    pub namespace: &'static str,
    /// The generated per-key accessor base name, e.g. `"profile"` →
    /// `profile(pubkey) -> ProfileCard?`. Always a valid lowerCamelCase ident.
    pub accessor: &'static str,
    /// `TypedPayload.schema_id` the producer stamps on the keyed projection.
    pub schema_id: &'static str,
    /// FlatBuffers `file_identifier` of the row-delta batch payload (`NRRD`).
    pub file_identifier: &'static str,

    /// ADR-0063 Lane C (#1671) — the TYPED ROW-PAYLOAD shape carried inside each
    /// `Changed` row, which turns the host accessor into a concrete domain type
    /// (`profile(pubkey) -> ProfileCard?`) instead of Lane A's raw `Data?`, and
    /// is what the decode-before-commit seam (invariant #2) validates against. It
    /// reuses an EXISTING typed-projection schema verbatim (NO new `.fbs`): see
    /// [`RefRowPayload`] for the per-namespace reader/glue mapping.
    pub row_payload: RefRowPayload,
}


/// The keyed reference projections (ADR-0063 / #1671). Ship `profile` + `event`
/// only (issue #1671 scope limit — no speculative namespaces).
pub const KEYED_PROJECTIONS: &[KeyedProjectionEntry] = &[
    KeyedProjectionEntry {
        projection_key: "refs.profile",
        namespace: "profile",
        accessor: "profile",
        schema_id: "nmp.refs.rowdelta",
        file_identifier: "NRRD",
        // Lane C: row payload is the EXISTING `KPRF` `ProfileSnapshot` buffer
        // (`Kernel::ref_profile_row_payload` → `encode_profile`); accessor returns
        // a decoded `ProfileCard?`. `profile.ref` narrows host-side exactly as the
        // Rust narrowing does (dropped fields decode nil). Reuses the existing
        // `TypedProjectionGlue.profile` (same buffer as the whole-value projection).
        row_payload: RefRowPayload {
            row_file_identifier: "KPRF",
            swift_reader_type: "nmp_kernel_ProfileSnapshot",
            swift_domain_type: "ProfileCard",
            swift_glue: "profile",
            kotlin: None,
        },
    },
    KeyedProjectionEntry {
        projection_key: "refs.event",
        namespace: "event",
        accessor: "event",
        schema_id: "nmp.refs.rowdelta",
        file_identifier: "NRRD",
        // Lane C: row payload is the EXISTING `KCEV` `ClaimedEventsSnapshot` buffer
        // with EXACTLY ONE entry (`Kernel::ref_event_row_payload` →
        // `encode_claimed_events`); accessor unwraps it to `ClaimedEventDto?` via
        // the Lane-C `refRowEvent` glue. `event.embed` omits the content-tree
        // bytes, `event.raw` carries them — both decode through the same reader.
        row_payload: RefRowPayload {
            row_file_identifier: "KCEV",
            swift_reader_type: "nmp_kernel_ClaimedEventsSnapshot",
            swift_domain_type: "ClaimedEventDto",
            swift_glue: "refRowEvent",
            kotlin: None,
        },
    },
];
