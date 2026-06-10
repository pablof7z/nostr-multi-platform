//! Typed FlatBuffers wire codecs for `nmp-nip02` projection nouns.
//!
//! The serde JSON shape registered via `register_snapshot_projection` (under
//! the key `"nmp.follow_list"`) stays authoritative; this codec adds the
//! typed-sidecar (ADR-0037) counterpart, emitted alongside the generic `Value`
//! tree in every `SnapshotFrame`.

pub mod typed_fb;

pub use typed_fb::{
    decode_follow_list, encode_follow_list, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION,
};
