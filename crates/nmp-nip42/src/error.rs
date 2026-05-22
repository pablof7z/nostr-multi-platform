//! NIP-42 typed error vocabulary shared by [`crate::builder`] and
//! [`crate::flow`].
//!
//! Lives in its own module so the structural validator in `builder.rs` can
//! return the same typed error the FSM in `flow.rs` consumes, without
//! either side wrapping a peer's `String` (D6 — one error type per crate,
//! no mixed `String`/`Box<dyn Error>` returns at the public boundary).

/// Errors the NIP-42 driver returns from its internal flow. Never crosses
/// FFI per D6 — converts to `RelayAuthState::Failed` plus a reason in
/// [`crate::flow::HandshakeOutcome`].
#[derive(Clone, Debug)]
pub enum Nip42Error {
    /// The signer was invoked but reported failure or unavailability.
    SignerFailed(String),
    /// The signer returned a structurally invalid event (wrong kind,
    /// missing challenge echo, malformed id, etc.). Catches buggy or
    /// malicious signers.
    SignerReturnedInvalid(String),
}

impl std::fmt::Display for Nip42Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignerFailed(m) => write!(f, "signer failed: {m}"),
            Self::SignerReturnedInvalid(m) => write!(f, "signer returned invalid event: {m}"),
        }
    }
}

impl std::error::Error for Nip42Error {}
