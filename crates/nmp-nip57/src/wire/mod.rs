//! Typed FlatBuffers wire codecs for `nmp-nip57` projection nouns and action
//! payloads (ADR-0037, ADR-0064).
//!
//! The serde JSON shapes registered via `register_snapshot_projection` stay
//! authoritative; these codecs add the typed-sidecar (ADR-0037) counterpart,
//! emitted alongside the generic `Value` tree in every `SnapshotFrame`.
//!
//! `zap_payload` holds the WRITE-direction [`nmp_core::substrate::ActionPayload`]
//! impl for [`crate::ZapInput`] (S9 #1747).

pub mod typed_fb;
pub mod zap_payload;

pub use typed_fb::{
    decode_zaps_snapshot, encode_zaps_snapshot, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION,
};
