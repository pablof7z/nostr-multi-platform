//! Typed FlatBuffers wire codecs for the NIP-17 snapshot projections.
//!
//! Each submodule is a sidecar codec (ADR-0037) for one projection: it encodes
//! the projection's read model into a schema-versioned, language-neutral
//! FlatBuffers buffer carried in the snapshot frame's `typed_projections`
//! sidecar, alongside — never replacing — the existing generic
//! `serde_json::Value` projection registered under the same key.
//!
//! - [`dm_inbox_fb`] — `"nmp.nip17.dm_inbox"` (`NDMI`).
//! - [`dm_relay_list_fb`] — `"nmp.nip17.dm_relay_list"` (`NDRL`).

pub mod dm_inbox_fb;
pub mod dm_relay_list_fb;
