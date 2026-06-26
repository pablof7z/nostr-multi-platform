//! Errors produced by the NIP-46 handshake state machine.
//!
//! Display strings flow directly to the broker's
//! `BunkerHandshakeProgress { stage: "failed", message }` so they must be
//! human-legible (D6 — no internal stack traces in user-facing strings).

/// Errors produced by the handshake state machine. Display strings flow
/// directly to `BunkerHandshakeProgress { stage: "failed", message }`.
#[derive(Debug, Clone)]
pub enum HandshakeError {
    /// Cancelled via `BunkerBroker::cancel`.
    Cancelled,
    /// Overall handshake deadline elapsed.
    Timeout(String),
    /// The bunker returned an explicit error response.
    BunkerError(String),
    /// Crypto / serialisation / parsing failure.
    Protocol(String),
    /// Relay write / transport error.
    Transport(String),
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Timeout(s) => write!(f, "timeout: {s}"),
            Self::BunkerError(s) => write!(f, "bunker error: {s}"),
            Self::Protocol(s) => write!(f, "protocol error: {s}"),
            Self::Transport(s) => write!(f, "transport error: {s}"),
        }
    }
}

impl std::error::Error for HandshakeError {}
