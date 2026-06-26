//! # nmp-nip46-runtime
//!
//! Actor-lane runtime for the NIP-46 remote-signer protocol (Layer-4).
//!
//! This crate drives the transport-agnostic [`nmp_nip46::SessionState`] reducer
//! over the NMP actor relay lane.  It is the NIP-46 equivalent of
//! `nmp-nip47` (NWC wallet runtime): one substrate-registered interceptor +
//! connected hook handle the full handshake and steady-state RPC lifecycle.
//!
//! ## PR-A dormancy contract
//!
//! In PR-A the broker (`nmp-signer-broker`) still drives the NIP-46
//! handshake; this crate is built and unit-tested but NOT wired into
//! `nmp-ffi` yet.  PR-B performs the flip: calls [`register_nip46`] from
//! the FFI initializer and deletes the broker transport.
//!
//! ## Architectural overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ nmp-nip46-runtime  (Layer-4 — mirrors nmp-nip47)                │
//! │                                                                 │
//! │  register_nip46(app, command_sender) → Nip46RuntimeHandle       │
//! │         │                                                       │
//! │         ├─ Nip46Interceptor  (RelayTextInterceptor)             │
//! │         │    · on_relay_text → SessionState::on_relay_text      │
//! │         │    · on_idle_tick  → SessionState::tick (60 s gate)   │
//! │         │    · translates Effect → OutboundMessage / ActorCmd   │
//! │         │                                                       │
//! │         └─ Nip46ConnectedHook  (RelayConnectedHook)             │
//! │              · on_relay_connected → SessionState::on_relay_connected │
//! │              · enqueue_outbound(REQ) via CommandSender          │
//! │              · REQ-before-EVENT channel FIFO guarantee          │
//! └─────────────────────────────────────────────────────────────────┘
//!          │
//!          ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ nmp-nip46  (pure reducer, no I/O, no threads)                   │
//! │   SessionState · Effect · start_bunker · start_nostrconnect     │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Relay-lifetime contract
//!
//! `RelayRole::Signer` is added to `nmp-network` and treated as persistent by
//! `nmp-core::relay_transport::relay_socket_is_persistent` (same contract as
//! `RelayRole::Wallet` for NWC).  The idle sweeper never reaps the bunker
//! socket between RPC calls.
//!
//! ## Key types
//!
//! - [`runtime::Nip46Runtime`] — session state + relay URL + sub_id + keys.
//! - [`runtime::Nip46RuntimeHandle`] — `Arc<Mutex<Option<Nip46Runtime>>>` shared
//!   by the interceptor, connected hook, and transport.
//! - [`register::register_nip46`] — config-phase registration function.
//! - [`transport::ActorLaneTransport`] — fire-and-forget [`Nip46Transport`] impl.

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![allow(clippy::module_name_repetitions)]

pub mod connected_hook;
pub mod interceptor;
pub mod register;
pub mod runtime;
pub mod transport;

// ─── flat re-exports (the public surface) ────────────────────────────────────

pub use register::register_nip46;
pub use runtime::{
    clear_runtime, init_bunker, init_nostrconnect, init_restore, new_nip46_runtime_handle,
    Nip46Runtime, Nip46RuntimeHandle,
};
pub use transport::ActorLaneTransport;

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
