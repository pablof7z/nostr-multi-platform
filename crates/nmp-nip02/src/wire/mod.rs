//! Typed FlatBuffers wire codecs for `nmp-nip02`.
//!
//! * [`typed_fb`] — the READ-direction projection sidecar (ADR-0072): the serde
//!   JSON `"nmp.follow_list"` snapshot stays authoritative and this typed
//!   sidecar rides alongside it in every `SnapshotFrame`.
//! * [`action_payload`] — the WRITE-direction action payloads (ADR-0071 / S3
//!   #1751): the `ActionPayload` impls for `PubkeyAction` (follow/unfollow) and
//!   `FollowManyAction` (follow_many), decoded by the registry adapter.

pub mod action_payload;
pub mod typed_fb;

pub use typed_fb::{
    decode_follow_list, encode_follow_list, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION,
};
