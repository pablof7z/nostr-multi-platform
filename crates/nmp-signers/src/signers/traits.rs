//! Core `Signer` trait and supporting types.
//!
//! See [`Signer`] for the contract.  See ADR-0015 for the design rationale.

use std::fmt::Debug;

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::PublicKey;
use serde::{Deserialize, Serialize};

use super::SignerOp;
use super::SignerPayload;

/// Backend kind for a [`Signer`].
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum SignerBackend {
    /// Local raw secret key (in-memory or NIP-49-at-rest).
    LocalKey,
    /// NIP-46 bunker:// remote signer.
    Nip46,
    /// NIP-07 browser extension (`window.nostr.*`).
    Nip07,
    /// Custom signer kind.
    Custom(String),
}

/// Error returned by every fallible [`Signer`] operation.
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
            SignerError::Backend(m) => write!(f, "backend error: {m}"),
        }
    }
}

impl std::error::Error for SignerError {}

/// Optional NIP-04 encrypt/decrypt namespace.
pub trait Nip04: Send + Sync {
    /// Encrypt `plaintext` for `recipient` using NIP-04.
    fn encrypt(&self, recipient: &PublicKey, plaintext: &str) -> SignerOp<String>;
    /// Decrypt `ciphertext` from `sender` using NIP-04.
    fn decrypt(&self, sender: &PublicKey, ciphertext: &str) -> SignerOp<String>;
}

/// Optional NIP-44 encrypt/decrypt namespace.
pub trait Nip44: Send + Sync {
    /// Encrypt `plaintext` for `recipient` using NIP-44 v2.
    fn encrypt(&self, recipient: &PublicKey, plaintext: &str) -> SignerOp<String>;
    /// Decrypt `payload` from `sender` using NIP-44 v2.
    fn decrypt(&self, sender: &PublicKey, payload: &str) -> SignerOp<String>;
}

/// The Signer contract.
///
/// ## Invariants
///
/// 1. `pubkey()` is **synchronous and infallible after construction succeeds**.
///    Constructors that require an async handshake (NIP-46) must complete that
///    handshake before returning `Ok`; see [`Nip46SignerHandle`] for the
///    pre-handshake handle type.
/// 2. `sign()` returns a signature whose embedded pubkey equals `self.pubkey()`
///    and whose computed id matches the unsigned template.  `AccountManager`
///    enforces this post-condition (applesauce SignerMismatchError).
/// 3. `nip04()` / `nip44()` return `Some(_)` iff the signer can service that
///    namespace.  Callers MUST check.
/// 4. `to_payload()` round-trips via the kind-specific constructor; re-handshake
///    may be required (NIP-46, NIP-07).
///
/// [`Nip46SignerHandle`]: super::Nip46SignerHandle
pub trait Signer: Send + Sync + Debug {
    /// Identify the backend kind.
    fn backend(&self) -> SignerBackend;

    /// Return the signer's public key.  Synchronous; cached after construction.
    fn pubkey(&self) -> PublicKey;

    /// Sign `unsigned`, returning a thunk that resolves to the signed event.
    fn sign(&self, unsigned: UnsignedEvent) -> SignerOp<SignedEvent>;

    /// Optional NIP-04 namespace.
    fn nip04(&self) -> Option<&dyn Nip04> {
        None
    }

    /// Optional NIP-44 namespace.
    fn nip44(&self) -> Option<&dyn Nip44> {
        None
    }

    /// Serialize the signer for persistence.  Round-trips via the kind-specific
    /// constructor.
    fn to_payload(&self) -> SignerPayload;
}
