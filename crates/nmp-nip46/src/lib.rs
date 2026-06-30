//! # nmp-nip46
//!
//! Transport-agnostic NIP-46 protocol core.
//!
//! This crate contains the pure protocol logic for NIP-46 remote-signer
//! connections: a pure-function event reducer (no threads, no blocking, no
//! `crossbeam`, no `SystemTime` on any reducer path), RPC helpers, and
//! wire-frame construction. It **spawns nothing**, **opens no sockets**, and
//! has no dependency on any NMP application or network layer.
//!
//! ## Module layout
//!
//! | module | contents |
//! |--------|----------|
//! | [`effect`] | [`effect::Effect`] + [`effect::SignerReady`] |
//! | [`reducer`] | [`reducer::SessionState`] — pure handshake state machine |
//! | [`bunker`] | [`bunker::start_bunker`] — client-initiated (`bunker://`) entry point |
//! | [`nostrconnect`] | [`nostrconnect::start_nostrconnect`] — signer-initiated entry point |
//! | [`error`] | [`error::HandshakeError`] |
//! | [`rpc`] | [`rpc::build_event_frame`], [`rpc::build_event_frame_at`], [`rpc::decode_inbound_response`] |
//! | [`restore`] | [`restore::start_restore`] — seed a Done-phase session from a saved payload |
//! | [`progress_codes`] | stable machine codes for progress labels |
//! | [`uri_encode`] | RFC 3986 query-value percent-encoder |

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![allow(clippy::module_name_repetitions)]

pub mod bunker;
pub mod effect;
pub mod error;
pub mod nostrconnect;
pub mod progress_codes;
pub mod reducer;
pub mod restore;
pub mod rpc;
pub mod uri_encode;

// ─── flat re-exports (the "nmp_nip46::" public surface) ──────────────────────

pub use bunker::start_bunker;
pub use effect::{Effect, SignerReady};
pub use error::HandshakeError;
pub use nostrconnect::start_nostrconnect;
pub use reducer::SessionState;
pub use restore::start_restore;
pub use rpc::{
    build_event_frame, build_event_frame_at, build_req_frame, decode_inbound_response, RpcBuildError,
};
pub use uri_encode::percent_encode_query_value;

#[cfg(test)]
mod tests;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
