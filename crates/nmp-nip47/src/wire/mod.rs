//! Typed FlatBuffers wire codecs for `nmp-nip47` nouns.
//!
//! - [`typed_fb`] — READ-direction `wallet_status` snapshot sidecar (ADR-0037):
//!   the serde JSON shape registered via `register_snapshot_projection` stays
//!   authoritative; this codec adds the typed counterpart emitted alongside the
//!   generic `Value` tree in every `SnapshotFrame`.
//! - [`action_payload`] — WRITE-direction typed action payloads (ADR-0064 /
//!   #1756) for `nmp.wallet.connect` (`N47C`), `nmp.wallet.disconnect` (`N47D`),
//!   and `nmp.wallet.pay_invoice` (`N47P`). The OPAQUE `DispatchEnvelope.payload`
//!   for each namespace; the registry adapter decodes them through
//!   [`nmp_core::substrate::ActionPayload::decode`] — the single typed-decode
//!   site — running the fail-closed `schema_version` gate BEFORE `start()`.

pub mod action_payload;
pub mod typed_fb;

pub use typed_fb::{
    decode_wallet_status, encode_wallet_status, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION,
};
