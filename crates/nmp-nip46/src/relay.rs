//! Transport seam: the only interface the protocol core requires of the outside
//! world.
//!
//! [`FrameSink`] is what the handshake and RPC helpers call to push a raw
//! NIP-01 wire frame (`["EVENT", ...]`) onto the relay. The production impl in
//! `nmp-signer-broker` bridges [`FrameSink`] → `RelayClient`; test stubs use a
//! `Vec`-backed impl. The protocol core never opens sockets, never spawns
//! threads, and never knows whether it is talking to one relay or many —
//! all of that is the broker's business.

/// Errors returned by a [`FrameSink`]. String-typed to keep the surface small;
/// the protocol core maps them to [`crate::error::HandshakeError::Transport`].
#[derive(Debug, Clone)]
pub struct FrameSinkError(pub String);

impl std::fmt::Display for FrameSinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FrameSinkError {}

/// The only outbound interface the NIP-46 protocol core requires. Implementors
/// push a fully-formed NIP-01 wire frame to the relay.
///
/// In production the broker wraps `nmp_signer_broker::RelayClient` behind this
/// trait so the handshake functions never import broker types. Test stubs
/// collect frames in a `Vec` for post-hoc assertions.
pub trait FrameSink: Send + Sync {
    /// Write a raw NIP-01 frame (`["EVENT", ...]`) to the relay. The frame is
    /// transient — implementations are NOT required to replay it after a
    /// reconnect. For subscription frames (`["REQ", ...]`) callers should use
    /// the broker's `RelayClient::subscribe` directly.
    fn send(&self, frame: String) -> Result<(), FrameSinkError>;
}
