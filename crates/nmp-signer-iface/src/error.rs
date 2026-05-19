//! [`SignerError`] — the canonical error type returned by every fallible
//! signer operation.
//!
//! String-typed by design — per doctrine **D6** errors never cross FFI as
//! exceptions; this type is for Rust-internal flow control only and gets
//! converted to `toast: Option<String>` at the FFI boundary.

use serde::{Deserialize, Serialize};

/// Error returned by every fallible signer operation.
///
/// String-typed by design — per doctrine **D6** errors never cross FFI as
/// exceptions; this type is for Rust-internal flow control only and gets
/// converted to `toast: Option<String>` at the FFI boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SignerError {
    /// Signer is in a state that cannot service the request (NIP-46 remote
    /// not yet connected, password-locked, etc.).
    NotReady(String),
    /// Signer does not support the requested operation (readonly signer asked
    /// to sign, encryption scheme unavailable, etc.).
    Unsupported(String),
    /// User-visible rejection (NIP-46 remote denied, extension user clicked
    /// "no", etc.).
    Rejected(String),
    /// Mismatch between expected and returned pubkey/id — catches malicious or
    /// buggy signers that mutate the event before signing (applesauce
    /// `SignerMismatchError`).
    Mismatch(String),
    /// Operation timed out.
    Timeout(String),
    /// Signature verification failed on a signed event returned by a remote
    /// signer.  Used by NIP-46 / NIP-07 to refuse responses whose id or
    /// signature do not validate under the claimed pubkey — protects against
    /// a compromised or malicious bunker returning a payload the local
    /// kernel would otherwise trust verbatim.
    SignatureVerificationFailed(String),
    /// Backend-specific failure (network, IO, parse, etc.).
    Backend(String),
}

impl std::fmt::Display for SignerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignerError::NotReady(m) => write!(f, "signer not ready: {m}"),
            SignerError::Unsupported(m) => write!(f, "unsupported: {m}"),
            SignerError::Rejected(m) => write!(f, "rejected: {m}"),
            SignerError::Mismatch(m) => write!(f, "signer mismatch: {m}"),
            SignerError::Timeout(m) => write!(f, "timeout: {m}"),
            SignerError::SignatureVerificationFailed(m) => {
                write!(f, "signature verification failed: {m}")
            }
            SignerError::Backend(m) => write!(f, "backend error: {m}"),
        }
    }
}

impl std::error::Error for SignerError {}
