//! # nmp-nip46-runtime
//!
//! Actor-lane runtime for the NIP-46 remote-signer protocol (Layer-4).
//!
//! This crate drives the transport-agnostic [`nmp_nip46::SessionState`] reducer
//! over the NMP actor relay lane.  It is the NIP-46 equivalent of
//! `nmp-nip47` (NWC wallet runtime): one substrate-registered interceptor +
//! connected hook handle the full handshake and steady-state RPC lifecycle.
//!
//! ## PR-B2: broker deleted, actor-lane is the sole NIP-46 transport
//!
//! `nmp-signer-broker` is deleted in PR-B2 (#2119). `register_nip46` is wired
//! into `nmp-native-runtime`'s `NmpApp::init_signer_broker` (the config-phase
//! entry point on the native composition-root app struct), and the
//! `ffi_support` module provides composition-boundary helpers that keep
//! `nmp-native-runtime`'s `signer_broker` module free of `RelayRole` /
//! `ActorLaneTransport` naming.
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
/// Composition-boundary helpers — wrap `RelayRole` / `ActorLaneTransport` so
/// that `nmp-native-runtime` (which must NOT name `nmp-network` on the
/// `signer-broker` feature path) can still deliver init effects, cancel
/// sessions, and restore from payload.
pub mod ffi_support;
pub mod interceptor;
pub mod register;
pub mod runtime;
pub mod transport;

// ─── flat re-exports (the public surface) ────────────────────────────────────

pub use ffi_support::{
    cancel_nip46_session, deliver_init_effects, make_sub_id, restore_nip46_from_payload,
};
pub use register::register_nip46;
pub use runtime::{
    clear_runtime, complete_signer_from_ready, init_bunker, init_nostrconnect, init_restore,
    mark_persistent_sub_registered, new_nip46_runtime_handle, record_signer_ready,
    take_persistent_registration, Nip46Runtime, Nip46RuntimeHandle,
};
pub use transport::ActorLaneTransport;

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
