//! Typed FlatBuffers action-payload codecs for the three relay-list action
//! modules this crate owns (ADR-0064 / #1756 — WRITE direction):
//!
//! - [`action_payload`] — `nmp.nip51.block_relay` (`NBLK`),
//!   `nmp.nip51.unblock_relay` (`NUBL`), and `nmp.nip65.publish_relay_list`
//!   (`N65P`).
//!
//! These are the typed payloads carried as the OPAQUE `DispatchEnvelope.payload`
//! for each namespace. The registry adapter decodes them through
//! [`nmp_core::substrate::ActionPayload::decode`] — the single typed-decode site
//! — running the fail-closed `schema_version` gate BEFORE `start()`.

pub mod action_payload;
