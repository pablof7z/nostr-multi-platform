//! App-neutral events emitted by the bunker broker.
//!
//! The broker owns transport and handshake lifecycle. Host composition owns
//! app policy: it receives these events and decides how to translate them into
//! actor commands, UI progress, or tests.

use std::sync::Arc;

use nmp_signers::Nip46Signer;

/// A completed broker outcome or progress update.
#[derive(Clone, Debug)]
pub enum BrokerEvent {
    /// Handshake progress suitable for a host-owned progress surface.
    Progress {
        /// Stage label such as `"connecting"`, `"awaiting_pubkey"`,
        /// `"ready"`, or `"failed"`.
        stage: String,
        /// Optional host-displayable detail.
        message: Option<String>,
    },
    /// A fully handshaken NIP-46 signer ready for host registration.
    SignerReady {
        /// Strong reference retained by the host adapter. The broker keeps
        /// its own session reference so cancellation can drain pending RPCs.
        signer: Arc<Nip46Signer>,
    },
    /// The relay-layer connection state changed. Emitted when the underlying
    /// `PoolRelayClient` observes a `Opened`, `Closed`, or `Failed` event from
    /// the `nmp-network` Pool. V-14 step b: gives the host visibility into
    /// mid-session relay flaps so the UI can display a reconnecting indicator
    /// or prompt re-auth rather than silently bricking the session.
    ///
    /// `state` is one of: `"connected"`, `"reconnecting"`, `"failed"`.
    /// `reason` carries the error message for `"reconnecting"` and `"failed"`.
    ConnectionStateChanged {
        /// Current relay-layer connection state token.
        state: String,
        /// Optional human-readable reason (error message on disconnect).
        reason: Option<String>,
    },
}

/// Callback installed by the host adapter that receives broker events.
pub type BrokerEventHandler = dyn Fn(BrokerEvent) + Send + Sync + 'static;
