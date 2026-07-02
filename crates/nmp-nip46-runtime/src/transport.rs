//! Actor-lane [`Nip46Transport`] implementation.
//!
//! [`ActorLaneTransport`] implements [`Nip46Transport`] for the actor-relay
//! lane.  Instead of writing directly to a WebSocket (as `BrokerTransport`
//! does), it:
//!
//! 1. Builds the fully-formed `["EVENT", ...]` kind:24133 wire frame
//!    (NIP-44 V2 encrypted + signed) using the shared frame builder from
//!    `nmp-nip46`.
//! 2. Posts `ActorCommand::EnqueueOutbound` via the captured
//!    [`CommandSender`] to **every** relay in `relay_urls` — fire-and-forget,
//!    non-blocking, never holds the runtime mutex.
//! 3. Returns `Ok(())` immediately.
//!
//! ## Multi-relay fan-out
//!
//! The transport fans each RPC to ALL relay URLs in its list.  This mirrors
//! the broker's multi-relay support (`BunkerUri::relays`).  A single-relay
//! bunker URI produces a list of length 1, so the common case is unchanged.
//!
//! ## Fire-and-forget vs sync error return
//!
//! The original `BrokerTransport::send_rpc` returned a synchronous socket-
//! write failure.  The actor-lane path is "accepted for delivery": the frame
//! is queued in the actor inbox and the actor thread sends it through
//! `send_outbound`.  A failed socket write surfaces as a later
//! `PoolEvent::Closed` (no-op for the handshake path) and the relay
//! reconnects.  Cryptographic build failures (key error, NIP-44 encrypt)
//! still return `Err` synchronously since they indicate a programmer error.
//!
//! ## Mutex-safety
//!
//! `ActorLaneTransport` holds NO runtime mutex.  The interceptor's
//! `on_relay_text` and the transport's `send_rpc` can run concurrently
//! without deadlock.

use nmp_core::CommandSender;
use nmp_network::role::RelayRole;
use nmp_nip46::build_event_frame;
use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError};
use nostr::{Keys, PublicKey};

/// Actor-lane transport for NIP-46 sign RPCs.
///
/// Fans each outbound RPC to ALL relay URLs in `relay_urls`.  `Clone`-able so
/// tests can hand clones to the actor and the test harness without lock
/// contention.
#[derive(Debug, Clone)]
pub struct ActorLaneTransport {
    sender: CommandSender,
    local_keys: Keys,
    remote_pubkey: PublicKey,
    /// All bunker relay URLs — every outbound EVENT is sent to every relay.
    relay_urls: Vec<String>,
}

impl ActorLaneTransport {
    /// Construct a new single-relay transport (common case).
    ///
    /// - `sender`: the actor's waking-inbox sender (ADR-0072 §D3a).
    /// - `local_keys`: the session's ephemeral keypair (NIP-44 + event signing).
    /// - `remote_pubkey`: the remote signer's public key (NIP-44 recipient +
    ///   `["p", ...]` tag on the kind:24133 event).
    /// - `relay_url`: the single bunker relay URL.
    #[must_use]
    pub fn new(
        sender: CommandSender,
        local_keys: Keys,
        remote_pubkey: PublicKey,
        relay_url: String,
    ) -> Self {
        Self {
            sender,
            local_keys,
            remote_pubkey,
            relay_urls: vec![relay_url],
        }
    }

    /// Construct a multi-relay transport.
    ///
    /// Each call to [`Self::send_rpc`] fans the encrypted EVENT frame to ALL
    /// URLs in `relay_urls`.  If `relay_urls` is empty the RPC is a no-op
    /// (accepted but never delivered — the session has no relay to route to).
    #[must_use]
    pub fn new_multi(
        sender: CommandSender,
        local_keys: Keys,
        remote_pubkey: PublicKey,
        relay_urls: Vec<String>,
    ) -> Self {
        Self {
            sender,
            local_keys,
            remote_pubkey,
            relay_urls,
        }
    }

    /// The relay URLs this transport fans outbound frames to.
    pub fn relay_urls(&self) -> &[String] {
        &self.relay_urls
    }
}

impl Nip46Transport for ActorLaneTransport {
    /// Encrypt `rpc.body_json_to_encrypt`, wrap in a kind:24133 event, sign,
    /// and post to the actor inbox as `EnqueueOutbound` for EACH relay URL.
    ///
    /// Returns `Err` only on cryptographic failures (NIP-44 encrypt, sign).
    /// Socket-write errors surface asynchronously via `PoolEvent::Closed`.
    fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
        let frame = build_event_frame(
            &self.local_keys,
            self.remote_pubkey,
            &rpc.body_json_to_encrypt,
        )
        .map_err(|e| SignerError::Backend(e.to_string()))?;

        for relay_url in &self.relay_urls {
            self.sender
                .enqueue_outbound(RelayRole::Signer, relay_url.clone(), frame.clone());
        }
        Ok(())
    }
}
