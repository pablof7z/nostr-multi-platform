//! Effects emitted by the NIP-46 handshake state machine.
//!
//! An [`Effect`] is an instruction the state machine emits and the caller
//! executes. The reducer itself is pure (no I/O, no threads, no blocking);
//! every side-effectful action is lifted out into this enum so the caller
//! can drive the protocol with any transport back-end — including a wasm
//! runtime that has no threads or `std::time`.

use crate::error::HandshakeError;

/// An effect that the caller must execute after the reducer returns.
///
/// Effects are returned in a `Vec` from every reducer entry point. The caller
/// (e.g. `nmp-signer-broker`) processes them in order: `Subscribe` → wire the
/// REQ frame; `SendFrame` → write an EVENT frame; `Progress` → forward to the
/// host UI; `SignerReady` → build the signer and complete the session;
/// `Error` → surface to the host as a handshake failure.
#[derive(Debug)]
pub enum Effect {
    /// Send a `["REQ", ...]` subscription frame to the relay identified by
    /// `relay_url`. The caller should use the transport's `subscribe()`
    /// method (not plain `send()`) so the frame survives reconnects (V-14).
    Subscribe {
        /// The relay URL this subscription targets.
        relay_url: String,
        /// The fully-formed `["REQ", sub_id, filter]` wire frame.
        frame: String,
    },
    /// Send a `["EVENT", ...]` wire frame to the relay.
    SendFrame {
        /// The relay URL to send to.
        relay_url: String,
        /// The fully-formed `["EVENT", <event>]` wire frame.
        text: String,
    },
    /// User-facing handshake progress update.
    Progress {
        /// Stage label (e.g. `"connecting"`, `"awaiting_pubkey"`).
        stage: String,
        /// Optional stable machine code for i18n/UI keying.
        code: Option<String>,
        /// Optional human-readable detail string.
        detail: Option<String>,
    },
    /// Handshake completed successfully. The caller should build the signer
    /// and emit `BrokerEvent::SignerReady`.
    SignerReady(SignerReady),
    /// Deliver a steady-state RPC response (post-handshake; reserved for
    /// future use in a full-reducer mode).
    DeliverResponse {
        /// The RPC request id that this response matches.
        correlation_id: String,
        /// Decrypted plaintext result.
        result: String,
    },
    /// Terminal error — the handshake failed. No further events from this
    /// session are expected.
    Error {
        /// The underlying error.
        error: HandshakeError,
    },
}

/// Payload of a successful NIP-46 handshake. Plain data — no nmp-signers
/// types — so `nmp-nip46` stays dep-clean.
#[derive(Debug, Clone)]
pub struct SignerReady {
    /// The user's pubkey hex (returned by `get_public_key`).
    pub user_pubkey_hex: String,
    /// The remote signer's pubkey hex (from the bunker URI, or learned from
    /// the signer's `connect` event in the `nostrconnect://` flow).
    pub remote_signer_pubkey_hex: String,
    /// Permissions that were requested/granted (pass-through from the
    /// originating broker call; `None` if not applicable).
    pub granted_perms: Option<String>,
}
