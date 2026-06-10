//! Typed FlatBuffers wire codecs for `nmp-marmot`'s snapshot projections.
//!
//! Sidecar to the authoritative serde JSON projections: the generic `Value`
//! shape stays the source of truth (the dynamic
//! `register_snapshot_projection` calls in `crate::ffi::register_with_keys`
//! are unchanged), and these modules carry the typed payloads in each
//! `SnapshotFrame`'s `typed_projections` sidecar (ADR-0037). Purely additive —
//! a host with the matching decoder prefers the typed payload; an un-updated
//! host falls back to the generic `Value` subtree.
//!
//! Two projections, two codecs (mirroring the nip29 two-projection layout):
//!   * [`snapshot_fb`] — `nmp.marmot.snapshot` (`NMMS`)
//!   * [`messages_fb`] — `nmp.marmot.messages` (`NMMG`)

pub mod messages_fb;
pub mod snapshot_fb;
