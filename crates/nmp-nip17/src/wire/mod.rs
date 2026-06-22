//! Typed FlatBuffers wire codecs for the NIP-17 snapshot projections and
//! action payloads.
//!
//! Snapshot projection sidecars (ADR-0037 / READ-direction):
//! - [`dm_inbox_fb`] — `"nmp.nip17.dm_inbox"` (`NDMI`).
//! - [`dm_relay_list_fb`] — `"nmp.nip17.dm_relay_list"` (`NDRL`).
//!
//! Action payload codecs (ADR-0064 / S9 #1747 / WRITE-direction):
//! - [`action_payload`] — `nmp.nip17.send` (`N17S`) and
//!   `nmp.nip17.publish_relay_list` (`N17R`).

pub mod action_payload;
pub mod dm_inbox_fb;
pub mod dm_relay_list_fb;
