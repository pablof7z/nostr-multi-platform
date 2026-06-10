//! Typed FlatBuffers wire codecs for `nmp-nip57` projection nouns.
//!
//! The serde JSON shapes registered via `register_snapshot_projection` stay
//! authoritative; these codecs add the typed-sidecar (ADR-0037) counterpart,
//! emitted alongside the generic `Value` tree in every `SnapshotFrame`.

pub mod typed_fb;

pub use typed_fb::{
    decode_zaps_snapshot, encode_zaps_snapshot, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION,
};
