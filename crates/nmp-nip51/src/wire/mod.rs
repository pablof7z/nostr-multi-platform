//! Typed FlatBuffers wire codecs for the NIP-51 snapshot projections.
//!
//! Each submodule is a sidecar codec (ADR-0037) for one projection: it encodes
//! the projection's read model into a schema-versioned, language-neutral
//! FlatBuffers buffer carried in the snapshot frame's `typed_projections`
//! sidecar, alongside — never replacing — the existing generic
//! `serde_json::Value` projection registered under the same key.
//!
//! - [`mute_list_fb`] — `"nmp.nip51.mute_list"` (`NMUT`).
//! - [`bookmark_update_fb`] — WRITE-direction typed action payload for
//!   `"nmp.nip51.add_bookmark"` / `"nmp.nip51.remove_bookmark"` (`N51B`); the
//!   single shared `BookmarkUpdateInput` codec (ADR-0064 / S9).

pub mod bookmark_update_fb;
pub mod mute_list_fb;
