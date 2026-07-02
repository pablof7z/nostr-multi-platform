//! V6 Stage 2 — `SnapshotProjections` dotted-projection-key registry.
//!
//! This module owns the single source of truth that replaces the hand-written
//! `SnapshotProjections` struct + `CodingKeys` enum at the bottom of
//! `apps/chirp/ios/Chirp/Bridge/KernelBridge.swift`. The renderer in
//! [`crate::swift`] reads this slice and emits the equivalent Swift.
//!
//! ## Why the registry lives in `nmp-codegen`, not `nmp-core`
//!
//! The Stage 2 registry is a list of `(json_key, swift_field, swift_type)`
//! triples — there is no Rust type to reflect via `schemars` (unlike Stage 1).
//! The natural home would have been `nmp-core::codegen_schema` alongside
//! Stage 1, BUT the registry MUST name dotted host-registered keys like
//! `"nmp.nip29.group_events"`, `"nmp.nip17.dm_inbox"`.
//! Those substrings would trip D0 doctrine-lint (`nip29` / `nip17` / `nip57`
//! tokens forbidden in `nmp-core` per `crates/nmp-testing/bin/doctrine-lint/
//! rules/d0.rs`). The substrings are legitimate here because *they are the
//! actual JSON wire keys the iOS shell consumes* — they are not Rust nouns
//! inlined into the kernel.
//!
//! `nmp-codegen` is exempt from D0 (it is a host-side tool crate, not the
//! kernel substrate), so the registry compiles cleanly here. The schema dump
//! binary in `nmp-core` already stays D0-clean — Stage 1 ships `Metrics` /
//! `RelayStatus` etc. by their Rust type names alone.
//!
//! ## What is *not* in this registry
//!
//! - The per-projection-value types themselves (`WalletStatusData`,
//!   `BunkerHandshake`, `PublishQueueEntry`, etc.). Those remain hand-written
//!   in `KernelBridge.swift` and are Stage 3 work. The generated
//!   `SnapshotProjections` only references them by their Swift type name —
//!   the reader must declare them somewhere reachable in the same module.
//! - The decoder configuration. The iOS shell's `KernelHandle.decode`
//!   continues to set `JSONDecoder.keyDecodingStrategy = .convertFromSnakeCase`
//!   — every `CodingKeys` raw value in the rendered enum is therefore the
//!   *post-transform* key (see `post_convert_from_snake_case` in
//!   [`crate::swift`]).
//!
//! ## Maintenance contract
//!
//! When a new snapshot projection is registered in Rust:
//!
//! 1. Add a new [`SnapshotProjectionEntry`] to [`SNAPSHOT_PROJECTIONS`] with
//!    the kernel-emitted JSON key, the Swift property name, and the Swift
//!    value type.
//! 2. Run `cargo run -p nmp-core --features codegen-schema --bin
//!    dump_projection_schemas | cargo run -p nmp-codegen -- gen swift` to
//!    regenerate `KernelTypes.generated.swift`. The CI gate
//!    (`.github/workflows/codegen-drift.yml`) fails any PR that forgets.
//! 3. If the new key's *value* type is not already declared in
//!    `KernelBridge.swift` (or in a previous Stage of the generator), add
//!    the Swift `Decodable` mirror there too — that work is Stage 3.

// ADR-0063 (#1671): the KEYED reference registry (`KeyedProjectionEntry` +
// `KEYED_PROJECTIONS`) and its Lane-C typed ROW-PAYLOAD descriptors live in a
// sibling module so this file stays under its 500-LOC cap. Re-exported so the
// existing `swift_projections_registry::{KeyedProjectionEntry, KEYED_PROJECTIONS}`
// import paths (the keyed-cache generators) are unchanged.
#[path = "keyed_projection_row_payload.rs"]
mod keyed_projection_row_payload;
pub use keyed_projection_row_payload::{
    KeyedProjectionEntry, KotlinRefRowPayload, RefRowPayload, TsRefRowPayload, KEYED_PROJECTIONS,
};

/// One entry in the dotted-projection-key registry.
///
/// The hand-written `SnapshotProjections` declaration in
/// `apps/chirp/ios/Chirp/Bridge/KernelBridge.swift` is the byte-for-byte target
/// the renderer must reproduce. Every field on that struct corresponds to
/// exactly one entry here, in declaration order.
pub struct SnapshotProjectionEntry {
    /// The projection's identity — the kernel-emitted JSON key as it appears in
    /// the `projections` map AND the `TypedProjection.key` the producer
    /// publishes for the typed sidecar. For entries in this registry, that key
    /// is owned by the producing projection contract. App-owned OP-feed session
    /// keys are intentionally absent from this shared registry even though their
    /// payloads use the shared `nmp.note_feed.opfeed` / NNFS schema.
    ///
    /// #1723 (epic #1719): this is the SINGLE source of the projection's
    /// identity in this registry. It used to be spelled twice — once as
    /// `json_key` here and once as `TypedSidecar::key` — and both had to equal
    /// the [`crate::projection_contract::PROJECTION_CONTRACT`] row's `key`. The
    /// two duplicate spellings collapsed onto this one field, which is itself
    /// looked up against the contract by a fail-closed gate
    /// ([`crate::projection_contract::tests`]) so the registry's projection set
    /// can never drift from the contract's.
    ///
    /// Used to compute the `CodingKeys` raw value via Apple's
    /// `.convertFromSnakeCase` transform (split on `_` only — `.` is opaque).
    ///
    /// Examples:
    /// - `"wallet"` → no transform needed, post-transform is `"wallet"`.
    /// - `"action_stages"` → post-transform is `"actionStages"`.
    /// - `"nmp.nip29.group_events"` → post-transform is `"nmp.nip29.groupEvents"`
    ///   (the `.`-segments stay intact, only `group_events` camelises).
    pub key: &'static str,
    /// Swift property name on `SnapshotProjections`. Always lowerCamelCase.
    /// The renderer emits `let <swift_field>: <swift_type>?` on the struct
    /// and `case <swift_field>` (or `case <swift_field> = "<raw>"`) on the
    /// `CodingKeys` enum.
    pub swift_field: &'static str,
    /// Swift value type (without the trailing `?`). Every member of
    /// `SnapshotProjections` is Optional — the kernel omits keys when the
    /// projection is empty / not yet populated, and D1 forward-compat
    /// requires the shell tolerate that.
    ///
    /// Plain types pass through verbatim: `"WalletStatusData"`,
    /// `"GroupEventsSnapshot"`. Container types are written in their full
    /// Swift form: `"[PublishQueueEntry]"`, `"[String: [ActionStageEntry]]"`,
    /// `"[String: ProfileCard]"`, `"[String]"`. The renderer never
    /// composes these — what you write here is what appears on the line.
    pub swift_type: &'static str,

    /// Typed-FlatBuffer-sidecar identity for this projection, or `None` when
    /// the kernel does NOT emit a typed sidecar for this key (the JSON
    /// `payload` path is the only wire form).
    ///
    /// This is the V6 Stage 4 (consumer-side) addition: every projection now
    /// ships a typed FlatBuffer entry in the `SnapshotFrame.typed_projections`
    /// sidecar (ADR-0037/0044). The consumer-side decoder generated by
    /// [`crate::swift_typed_decoders`] locates the envelope by the entry's own
    /// `key` and needs only the `flatc --swift` reader struct name (to decode
    /// it) from here; the neutral `schema_id` / `file_identifier` it verifies
    /// against are sourced from the projection contract by the entry's `key`
    /// (#1723).
    ///
    /// Every entry in the registry MUST have `typed_sidecar: Some(...)`.
    /// A `None` value means the projection has no typed wire form and is
    /// therefore a JSON-era vestigial that should be removed from the registry.
    /// The `typed_sidecar_coverage_gate` test in `swift_projections_registry_tests.rs`
    /// enforces this invariant and will fail if any entry has `None`.
    ///
    /// `swift_reader_type: None` inside a `Some(TypedSidecar { ... })` is the
    /// acceptable interim state: the typed FlatBuffer sidecar exists on the wire
    /// but the `flatc --swift` binding has not yet been checked into the Chirp
    /// target. The generator skips those entries (no Swift decoder emitted) but
    /// they remain in the registry because the WIRE form is canonical.
    pub typed_sidecar: Option<TypedSidecar>,
}

/// Typed-FlatBuffer-sidecar PRESENTATION identity for one projection key — the
/// Swift-specific facts the consumer-side decoder generator needs that are NOT
/// neutral: the producer envelope `key` and the `flatc --swift` reader struct
/// name.
///
/// #1723 (epic #1719): the neutral `schema_id` / `file_identifier` fields were
/// REMOVED from this struct (they are owned by the neutral
/// [`crate::projection_contract::PROJECTION_CONTRACT`] row, looked up by the
/// owning entry's `key`), and so was the redundant `key` field — the producer
/// envelope key is the SAME string as the entry's kernel-emitted `key`, so it is
/// no longer spelled a second time here. This struct now carries ONLY the one
/// genuinely-Swift presentation fact the contract cannot hold: the `flatc
/// --swift` reader struct name. The host-decoder generator
/// ([`crate::swift_typed_decoders`]) sources the neutral facts from the contract
/// by the entry's `key`. The host-side sidecar consumer still matches an
/// envelope by `envelope.key == <entry key> && envelope.schemaId == <contract
/// schema_id>`, then decodes via `getCheckedRoot(fileId: <contract
/// file_identifier>)` into the `swift_reader_type` struct.
pub struct TypedSidecar {
    /// The `flatc --swift` generated reader struct name
    /// (`namespace`-prefixed: `nmp_kernel_AccountsSnapshot`,
    /// `nmp_nip47_WalletStatus`), or `None` when the `flatc --swift` binding
    /// for this schema has NOT yet been generated + checked into the Chirp
    /// target.
    ///
    /// Only **six** `flatc --swift` bindings ship in
    /// `apps/chirp/ios/Chirp/Bridge/Generated/` today (op_feed, timeline_snapshot,
    /// content_tree, feed_home, nmp_update — plus the two proof-key bindings
    /// this PR adds: `accounts`, `active_account`). The remaining ~29 sidecar
    /// schemas have no Swift reader yet, so their `swift_reader_type` is `None`
    /// and the generator emits NO typed decoder for them — referencing a type
    /// the Chirp target cannot see would not compile. Generating those
    /// bindings (+ a binding-drift gate) is the named follow-up that unblocks
    /// the full sweep.
    pub swift_reader_type: Option<&'static str>,
}

// The concrete registry rows live in a sibling module so this file stays
// under its 500-LOC cap (mirrors the `keyed_projection_row_payload` split
// above). Re-exported so the existing `swift_projections_registry::
// SNAPSHOT_PROJECTIONS` import path (every consumer in this crate) is
// unchanged.
#[path = "swift_projections_registry_entries.rs"]
mod entries;
pub use entries::SNAPSHOT_PROJECTIONS;

#[cfg(test)]
#[path = "swift_projections_registry_tests.rs"]
mod tests;
