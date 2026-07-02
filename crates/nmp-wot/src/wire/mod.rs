//! Typed FlatBuffers wire codecs for `nmp-wot`'s snapshot projections.
//!
//! Sidecar to the authoritative serde JSON projections: the generic `Value`
//! shape stays the source of truth, and these modules carry the typed payloads
//! in each `SnapshotFrame`'s `typed_projections` sidecar (ADR-0072).

pub mod typed_fb;
