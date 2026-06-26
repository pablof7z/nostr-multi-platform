//! # nmp-nip46
//!
//! Transport-agnostic NIP-46 protocol core.
//!
//! This crate contains the pure protocol logic for NIP-46 remote-signer
//! connections: handshake state machines, RPC helpers, and wire-frame
//! construction. It **spawns nothing**, **opens no sockets**, and has no
//! dependency on any NMP application or network layer.
//!
//! The only transport coupling is the [`relay::FrameSink`] trait — a single
//! `send(&str) -> Result<(), FrameSinkError>`. Production code in
//! `nmp-signer-broker` wraps its `RelayClient` behind this seam; test stubs
//! use `Vec`-backed impls.
//!
//! ## Module layout
//!
//! | module | contents |
//! |--------|----------|
//! | [`relay`] | [`relay::FrameSink`] trait + [`relay::FrameSinkError`] |
//! | [`error`] | [`error::HandshakeError`] |
//! | [`rpc`] | [`rpc::build_event_frame`], [`rpc::decode_inbound_response`], id gen |
//! | [`wait`] | blocking event-driven waits (STEP-1 carry; removed in STEP 2) |
//! | [`bunker`] | client-initiated handshake (`bunker://`) |
//! | [`nostrconnect`] | signer-initiated handshake (`nostrconnect://`) |
//! | [`progress_codes`] | stable machine codes for progress labels |
//! | [`uri_encode`] | RFC 3986 query-value percent-encoder |

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![allow(clippy::module_name_repetitions)]

pub mod bunker;
pub mod error;
pub mod nostrconnect;
pub mod progress_codes;
pub mod relay;
pub mod rpc;
pub mod uri_encode;
pub(crate) mod wait;

// ─── flat re-exports (the "nmp_nip46::" public surface) ──────────────────────

pub use bunker::{build_req_frame, run_handshake, HandshakeOutcome};
pub use error::HandshakeError;
pub use nostrconnect::{run_nostrconnect_handshake, NostrConnectOutcome};
pub use relay::{FrameSink, FrameSinkError};
pub use rpc::{build_event_frame, decode_inbound_response, RpcBuildError};
pub use uri_encode::percent_encode_query_value;

#[cfg(test)]
mod tests;
