//! Typed FlatBuffers wire codecs for the NIP-29 snapshot projections.
//!
//! Each submodule is a sidecar codec (ADR-0037) for one projection: it encodes
//! the projection's serde read model into a schema-versioned, language-neutral
//! FlatBuffers buffer carried in the snapshot frame's `typed_projections`
//! sidecar, alongside — never replacing — the existing generic `serde_json::Value`
//! projection registered under the same key.
//!
//! - [`group_chat_fb`] — `"nmp.nip29.group_chat"` (`NGCS`).
//! - [`discovered_groups_fb`] — `"nmp.nip29.discovered_groups"` (`NDGS`).
//! - [`group_defaults_fb`] — `"nmp.nip29.group_defaults"` (`NGDF`).
//! - [`joined_groups_fb`] — `"nmp.nip29.joined_groups"` (`NJGS`).

pub mod discovered_groups_fb;
pub mod group_chat_fb;
pub mod group_defaults_fb;
pub mod joined_groups_fb;
