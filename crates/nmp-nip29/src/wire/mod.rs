//! Typed FlatBuffers wire codecs for the NIP-29 snapshot projections.
//!
//! Each submodule is a sidecar codec (ADR-0037) for one projection: it encodes
//! the projection's serde read model into a schema-versioned, language-neutral
//! FlatBuffers buffer carried in the snapshot frame's `typed_projections`
//! sidecar, alongside — never replacing — the existing generic `serde_json::Value`
//! projection registered under the same key.
//!
//! - [`group_timeline_fb`] — `"nmp.nip29.group_timeline"` (`NGTL`).
//! - [`discovered_groups_fb`] — `"nmp.nip29.discovered_groups"` (`NDGS`).
//! - [`group_defaults_fb`] — `"nmp.nip29.group_defaults"` (`NGDF`).
//! - [`joined_groups_fb`] — `"nmp.nip29.joined_groups"` (`NJGS`).
//!
//! Separately, [`action_payload`] holds the WRITE-direction typed action
//! payload codecs (ADR-0064 / S9 #1747): the `ActionPayload` impls for every
//! event-authoring NIP-29 `ActionModule` (`join` / `leave` / `publish_group_event`
//! / `create_public_group` / `react_in_group` / `share_event_in_group` /
//! `repost_in_group` / `put_user` / `create_invite`). These are decoded by the
//! registry adapter through each module's `decode_payload` override — the
//! single typed-decode site — with the fail-closed `schema_version` gate run
//! BEFORE `start()`.

pub mod action_payload;
pub mod discovered_groups_fb;
pub mod group_timeline_fb;
pub mod group_defaults_fb;
pub mod joined_groups_fb;
