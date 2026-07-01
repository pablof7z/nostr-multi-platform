//! Typed FlatBuffers action-payload codec for the router-owned relay-list
//! action (ADR-0064 / #1756 — WRITE direction):
//!
//! - [`action_payload`] — `nmp.nip65.publish_relay_list` (`N65P`).
//!
//! This is the typed payload carried as the OPAQUE `DispatchEnvelope.payload`
//! for the namespace. The registry adapter decodes it through
//! [`nmp_core::substrate::ActionPayload::decode`] — the single typed-decode site
//! — running the fail-closed `schema_version` gate BEFORE `start()`.

pub mod action_payload;
