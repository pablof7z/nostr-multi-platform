//! Typed FlatBuffers wire codecs for `nmp-marmot`.
//!
//! ## Write-direction (ADR-0071 / #2169)
//!
//! [`action_payload`] — `nmp.marmot` (`NMMA`) — the typed
//! `DispatchEnvelope.payload` codec for Marmot's installed action namespace.
//! Implements [`nmp_core::substrate::ActionPayload`] for
//! [`crate::projection::action::MarmotAction`] so the byte doorway routes
//! `nmp.marmot` dispatches through the crate-owned decoder.
//!
//! ## Read-direction (ADR-0072 snapshot sidecars)
//!
//! Sidecar to the authoritative serde JSON projections: the generic `Value`
//! shape stays the source of truth, and these modules carry the typed payloads
//! in each `SnapshotFrame`'s `typed_projections` sidecar. Purely additive — a
//! host with the matching decoder prefers the typed payload; an un-updated host
//! falls back to the generic `Value` subtree.
//!
//!   * [`snapshot_fb`] — `nmp.marmot.snapshot` (`NMMS`)
//!   * [`messages_fb`] — `nmp.marmot.messages` (`NMMG`)

pub mod action_payload;
pub mod messages_fb;
pub mod snapshot_fb;
