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
pub mod relay_client;
pub mod transport;

/// Facade module: re-exports the NIP-46 handshake functions with the broker's
/// `&dyn RelayClient` signature. Internally bridges `RelayClient` →
/// `nmp_nip46::FrameSink` so all callers in `broker/*.rs` compile with ZERO
/// source edits — they keep using `crate::handshake::run_handshake(relay, ...)`
/// where `relay: &dyn RelayClient`.
pub mod handshake {
    use crossbeam_channel::Receiver;
    use nostr::{Keys, PublicKey};
    use serde_json::Value;

    pub use nmp_nip46::{
        build_event_frame, build_req_frame, decode_inbound_response, HandshakeError,
        HandshakeOutcome, NostrConnectOutcome,
    };

    /// Bridges the broker's `RelayClient` to `nmp_nip46::FrameSink`.
    /// Trait-object upcasting (`&dyn RelayClient → &dyn FrameSink`) is NOT
    /// automatic in stable Rust, so this one-line newtype fills the gap at the
    /// module boundary rather than at every call site.
    struct FrameSinkAdapter<'a>(&'a dyn crate::relay_client::RelayClient);

    impl nmp_nip46::FrameSink for FrameSinkAdapter<'_> {
        fn send(&self, frame: String) -> Result<(), nmp_nip46::FrameSinkError> {
            self.0
                .send(frame)
                .map_err(|e| nmp_nip46::FrameSinkError(e.to_string()))
        }
    }

    /// Run the client-initiated (`bunker://`) handshake. Preserves the
    /// original `&dyn RelayClient` signature so `broker/handshake_thread.rs`
    /// requires no edits.
    #[allow(clippy::too_many_arguments)]
    pub fn run_handshake(
        relay: &dyn crate::relay_client::RelayClient,
        inbound_rx: &Receiver<Value>,
        cancel_rx: &Receiver<()>,
        local_keys: &Keys,
        remote_pubkey: PublicKey,
        secret: Option<&str>,
        perms: Option<&str>,
        progress: &mut dyn FnMut(&str, &str, Option<&str>),
    ) -> Result<HandshakeOutcome, HandshakeError> {
        nmp_nip46::run_handshake(
            &FrameSinkAdapter(relay),
            inbound_rx,
            cancel_rx,
            local_keys,
            remote_pubkey,
            secret,
            perms,
            progress,
        )
    }

    /// Run the signer-initiated (`nostrconnect://`) handshake. Preserves the
    /// original `&dyn RelayClient` signature so `broker/nostrconnect.rs`
    /// requires no edits.
    pub fn run_nostrconnect_handshake(
        relay: &dyn crate::relay_client::RelayClient,
        inbound_rx: &Receiver<Value>,
        cancel_rx: &Receiver<()>,
        local_keys: &Keys,
        expected_secret: &str,
        progress: &mut dyn FnMut(&str, &str, Option<&str>),
    ) -> Result<NostrConnectOutcome, HandshakeError> {
        nmp_nip46::run_nostrconnect_handshake(
            &FrameSinkAdapter(relay),
            inbound_rx,
            cancel_rx,
            local_keys,
            expected_secret,
            progress,
        )
    }
}

/// Facade module: re-exports progress codes from `nmp-nip46` so callers using
/// `crate::progress_codes::SENDING_CONNECT_TO_BUNKER` etc. compile unchanged.
pub mod progress_codes {
    pub use nmp_nip46::progress_codes::*;
}

/// Facade module: re-exports the URI encoder from `nmp-nip46` so
/// `crate::uri_encode::percent_encode_query_value` in `broker/nostrconnect.rs`
/// compiles without any source edit.
pub mod uri_encode {
    pub use nmp_nip46::percent_encode_query_value;
}

pub use broker::BunkerBroker;
pub use events::{BrokerEvent, BrokerEventHandler, RelayIntakeDropReason};
pub use nmp_nip46::percent_encode_query_value;
pub use transport::BrokerTransport;

/// Opaque completion sink for steady-state NIP-46 responses (ADR-0050 §D3b).
///
/// When the broker's inbound dispatcher decrypts a kind:24133 RPC reply, it
/// hands the plaintext RPC body (`{"id":...,"result":...}`) to this sink
/// instead of resolving the signer's pending op directly on the dispatcher
/// thread. The host composition (nmp-ffi) installs a sink that sends a
/// `DeliverSignerResponse` actor command, so completion delivery rides the
/// single waking actor inbox and the pending-map mutation happens on the actor
/// thread (D4 single-writer).
///
/// `nmp-signer-broker` sees only this opaque `Fn(String)` — it never names
/// `ActorCommand` or any `nmp-core` type, keeping the broker D0-clean. The
/// handshake path is unaffected (it completes via the existing
/// `BrokerEvent::SignerReady` re-entry, not this sink).
pub type CompletionSink = std::sync::Arc<dyn Fn(String) + Send + Sync>;
