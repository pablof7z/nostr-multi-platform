//! App-neutral events emitted by the bunker broker.
//!
//! The broker owns transport and handshake lifecycle. Host composition owns
//! app policy: it receives these events and decides how to translate them into
//! actor commands, UI progress, or tests.

use std::sync::Arc;

use nmp_signers::Nip46Signer;

/// Why the broker intentionally dropped a relay intake event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayIntakeDropReason {
    /// The bounded intake was full, so the oldest queued event was discarded
    /// to admit a newer relay event.
    DroppedOldest,
    /// The bounded intake was still full after the broker tried to free one
    /// slot, so the newest relay event was discarded.
    DroppedNewest,
}

/// A completed broker outcome or progress update.
#[derive(Clone, Debug)]
pub enum BrokerEvent {
    /// Handshake progress suitable for a host-owned progress surface.
    Progress {
        /// Stage label such as `"connecting"`, `"awaiting_pubkey"`,
        /// `"ready"`, or `"failed"`.
        stage: String,
        /// Stable machine code for a user-facing progress label
        /// (`progress_codes::*`); `None` for diagnostic / `"failed"` transitions
        /// that carry raw upstream detail rather than curated copy (#1711). The
        /// shell localizes the code, falling back to `message` when absent.
        code: Option<String>,
        /// Optional host-displayable detail (the English fallback prose).
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
    /// Bounded relay intake diagnostic. Emitted only at coalesced cumulative
    /// drop counts so a hostile relay cannot flood the host event path.
    RelayIntakeDropped {
        /// Drop policy outcome for the event that was discarded.
        reason: RelayIntakeDropReason,
        /// Cumulative number of relay events intentionally dropped by this
        /// session-local intake queue.
        dropped_total: u64,
        /// Fixed capacity of the session-local relay intake queue.
        capacity: usize,
    },
}

/// Callback installed by the host adapter that receives broker events.
pub type BrokerEventHandler = dyn Fn(BrokerEvent) + Send + Sync + 'static;
