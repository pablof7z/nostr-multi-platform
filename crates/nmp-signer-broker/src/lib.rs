//! # nmp-signer-broker
//!
//! App-neutral NIP-46 bunker transport and handshake coordinator.
//!
//! This crate owns the reusable wire work: dialing bunker relays, running the
//! `connect` / `get_public_key` handshake, restoring persisted NIP-46
//! sessions, and carrying steady-state RPC traffic for `Nip46Signer`. It does
//! not know about `NmpApp`, C FFI, or actor commands. Host composition installs
//! a [`BrokerEventHandler`] and translates [`BrokerEvent`] values into its own
//! lifecycle.
//!
//! ## Responsibilities
//!
//! 1. **Handshake**: parse a `bunker://` URI, dial the first relay, run the
//!    `connect` + `get_public_key` RPC dance, learn the user's pubkey.
//! 2. **Hand-off**: once the user pubkey is known, construct a fully connected
//!    `Nip46Signer` and emit [`BrokerEvent::SignerReady`].
//! 3. **Steady-state transport**: implements [`nmp_signer_iface::Nip46Transport`]
//!    so the `Nip46Signer` can publish kind:24133 RPCs after handshake. The
//!    same persistent relay subscription routes inbound responses back to
//!    `Nip46Signer::resolve_response`.
//! 4. **Progress reporting**: emits [`BrokerEvent::Progress`] updates
//!    (`"connecting"` → `"awaiting_pubkey"` → `"ready"`, or `"failed"` on
//!    error) so the host UI can render live feedback.
//!    `"ready"` is the terminal success stage; no `"idle"` follow-up is
//!    emitted — once the new `signer_kind == "nip46"` account appears in the
//!    kernel snapshot, the host can dismiss its progress UI on its own
//!    schedule. Timer-driven cleanup belongs to the UI layer, not this crate
//!    (D8).
//!
//! ## Threading
//!
//! Each call to [`BunkerBroker::start_handshake`] spawns a worker thread that
//! owns the WebSocket and drives the protocol top-down. The actor thread is
//! never blocked: progress and the eventual signer-ready event arrive through
//! the callback supplied by the host adapter.
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
//! Nothing in this crate imports `nmp-core` or `nmp-ffi`. The C/actor adapter
//! lives in `nmp-ffi`: it registers the kernel's bunker hook, owns the
//! process-global broker, and translates [`BrokerEvent`] into actor commands.

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![allow(clippy::module_name_repetitions)]

pub mod broker;
pub mod events;
pub mod handshake;
pub mod relay_client;
pub mod transport;
mod uri_encode;

pub use broker::BunkerBroker;
pub use events::{BrokerEvent, BrokerEventHandler};
pub use transport::BrokerTransport;
pub use uri_encode::percent_encode_query_value;
