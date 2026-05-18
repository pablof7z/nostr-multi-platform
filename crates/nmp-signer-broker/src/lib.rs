//! # nmp-signer-broker
//!
//! Stage 4 of the NIP-46 wiring: this crate bridges `nmp-core` (kernel actor)
//! and `nmp-signers` (concrete `Nip46Signer` + handle types). It is the only
//! crate that depends on both — doctrine **D0** forbids `nmp-core` from
//! importing `nmp-signers`, so the broker lives outside `nmp-core` and
//! reaches back through the `nmp-core::bunker_hook` indirection.
//!
//! ## Responsibilities
//!
//! 1. **Handshake**: parse a `bunker://` URI, dial the first relay, run the
//!    `connect` + `get_public_key` RPC dance, learn the user's pubkey.
//! 2. **Hand-off**: once the user pubkey is known, construct a fully-connected
//!    `Nip46Signer` and ship it to the actor via
//!    [`nmp_core::ActorCommand::AddRemoteSigner`]. The actor will then route
//!    every `sign_active` call through the signer for the active account.
//! 3. **Steady-state transport**: implements [`nmp_signer_iface::Nip46Transport`]
//!    so the `Nip46Signer` can publish kind:24133 RPCs after handshake. The
//!    same persistent relay subscription routes inbound responses back to
//!    `Nip46Signer::resolve_response`.
//! 4. **Progress reporting**: emits [`nmp_core::ActorCommand::BunkerHandshakeProgress`]
//!    snapshots (`"connecting"` → `"awaiting_pubkey"` → `"ready"` → `"idle"`,
//!    or `"failed"` on error) so the SwiftUI sign-in flow can render live
//!    feedback. The Chirp `AccountsView` auto-dismisses the sheet when a new
//!    `signer_kind == "nip46"` account appears.
//!
//! ## Threading
//!
//! Each call to [`BunkerBroker::start_handshake`] spawns a worker thread that
//! owns the WebSocket and drives the protocol top-down. The actor thread is
//! never blocked: progress and the eventual `AddRemoteSigner` arrive through
//! `std::sync::mpsc::Sender` (cheap clone of the actor's command sender).
//!
//! ## Cancellation
//!
//! [`BunkerBroker::cancel`] sets a flag observed by the handshake loop. The
//! WebSocket read uses a short timeout so the loop wakes up promptly. MVP
//! supports one active session at a time; calling `start_handshake` while a
//! prior session is still running cancels the prior session first.
//!
//! ## D0 invariant
//!
//! Nothing in `nmp-core` imports anything from this crate. The wiring is via
//! the `bunker_hook` indirection: `nmp_signer_broker_init` calls
//! `nmp_core::register_bunker_hook(...)` with a closure that captures the
//! broker. The closure pushes work onto a worker thread and returns
//! immediately — the actor thread continues running.

#![deny(unsafe_code)]
#![warn(missing_docs)]
// `unsafe_code` is allowed only in the `ffi` module via a scoped override
// (the `*mut NmpApp` C ABI cannot be `unsafe` at the Rust level).
#![allow(clippy::module_name_repetitions)]

pub mod broker;
pub mod ffi;
pub mod handshake;
pub mod relay_client;
pub mod transport;

pub use broker::BunkerBroker;
pub use ffi::{
    nmp_app_cancel_bunker_handshake, nmp_app_nostrconnect_uri, nmp_broker_free_string,
    nmp_signer_broker_init,
};
pub use transport::BrokerTransport;
