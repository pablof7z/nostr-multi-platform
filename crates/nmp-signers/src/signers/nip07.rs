//! NIP-07 (browser extension) signer.
//!
//! Real `window.nostr.*` bindings require a wasm target which is not yet wired
//! into this workspace.  This module ships the trait shape + serialization
//! payload so the wasm follow-up is a pure additive change (no API churn).
//!
//! Non-wasm builds keep the payload + trait shape available.  Operations return
//! `Unsupported`; `pubkey()` is guaranteed by construction-time gating to
//! always return a real key — every constructor either supplies a cached
//! pubkey or returns `SignerError::NotReady`.  No panic path remains.

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::PublicKey;

use super::payload::{Nip07Payload, SignerPayload};
use super::traits::{Nip04, Nip44, Signer, SignerBackend, SignerError};
use super::SignerOp;

/// Browser-extension NIP-07 signer.
///
/// On non-wasm builds, every sign / encrypt / decrypt operation returns
/// [`SignerError::Unsupported`] with a clear "wasm target required" message.
/// `pubkey()` is synchronous and infallible because **construction is gated**:
/// a `Nip07Signer` cannot exist without a cached pubkey (per applesauce
/// `0867a502` — the extension's pubkey is cached after first handshake).
///
/// ## Construction
///
/// - [`Nip07Signer::from_cached_pubkey`] — restore an extension session with a
///   known pubkey (the common path; the caller has it from a previous payload
///   or from a fresh `window.nostr.getPublicKey()` round-trip).
/// - [`Nip07Signer::from_payload`] — restore from a persisted
///   [`Nip07Payload`].  Returns [`SignerError::NotReady`] if the payload
///   carries no cached pubkey — the caller must re-handshake on wasm before
///   the signer is usable.
///
/// ## Doctrine D6 compliance
///
/// `pubkey()` never panics and never returns `Result` (per trait invariant).
/// Failure modes are surfaced at construction as structured `SignerError`
/// values that callers can map to `toast: Option<String>` at the FFI boundary.
#[derive(Debug)]
pub struct Nip07Signer {
    cached_pubkey: PublicKey,
}

impl Nip07Signer {
    /// Construct from a known cached pubkey.  Always succeeds.
    ///
    /// This is the canonical construction path for both production (after a
    /// wasm `window.nostr.getPublicKey()` round-trip) and restore-from-storage
    /// flows.
    #[must_use] 
    pub fn from_cached_pubkey(pubkey: PublicKey) -> Self {
        Self {
            cached_pubkey: pubkey,
        }
    }

    /// Restore from a payload.
    ///
    /// Returns [`SignerError::NotReady`] if the payload carries no cached
    /// pubkey — restore is impossible without a wasm re-handshake.  This is
    /// the structured-error equivalent of the panic this module used to throw
    /// when `pubkey()` was called on an empty signer (D6: errors never cross
    /// FFI as panics).
    pub fn from_payload(p: &Nip07Payload) -> Result<Self, SignerError> {
        let hex = p.cached_pubkey_hex.as_deref().ok_or_else(|| {
            SignerError::NotReady(
                "nip07 payload has no cached pubkey; wasm handshake required \
                 (`window.nostr.getPublicKey()`) before signer is usable"
                    .to_string(),
            )
        })?;
        let pubkey = PublicKey::from_hex(hex).map_err(|e| {
            SignerError::Backend(format!("invalid cached nip07 pubkey hex: {e}"))
        })?;
        Ok(Self {
            cached_pubkey: pubkey,
        })
    }

    /// Whether the current build can actually talk to the extension.
    #[must_use] 
    pub const fn nip07_supported() -> bool {
        cfg!(all(target_arch = "wasm32", feature = "wasm"))
    }
}

impl Signer for Nip07Signer {
    fn backend(&self) -> SignerBackend {
        SignerBackend::Nip07
    }

    fn pubkey(&self) -> PublicKey {
        // Construction-gated: `cached_pubkey` is always set.  No panic path.
        self.cached_pubkey
    }

    fn sign(&self, _unsigned: UnsignedEvent) -> SignerOp<SignedEvent> {
        SignerOp::err(SignerError::Unsupported(
            "NIP-07 signing requires wasm target + browser extension; \
             enable feature = \"wasm\" and target wasm32-unknown-unknown"
                .to_string(),
        ))
    }

    fn nip04(&self) -> Option<&dyn Nip04> {
        // Extensions may or may not expose `nip04`; we return Some(self) so
        // callers get a clear runtime "Unsupported" rather than guessing.
        Some(self)
    }

    fn nip44(&self) -> Option<&dyn Nip44> {
        Some(self)
    }

    fn to_payload(&self) -> SignerPayload {
        SignerPayload::Nip07(Nip07Payload {
            cached_pubkey_hex: Some(self.cached_pubkey.to_hex()),
        })
    }
}

impl Nip04 for Nip07Signer {
    fn encrypt(&self, _recipient: &PublicKey, _plaintext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported(
            "NIP-07 nip04 encrypt: wasm target required".to_string(),
        ))
    }
    fn decrypt(&self, _sender: &PublicKey, _ciphertext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported(
            "NIP-07 nip04 decrypt: wasm target required".to_string(),
        ))
    }
}

impl Nip44 for Nip07Signer {
    fn encrypt(&self, _recipient: &PublicKey, _plaintext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported(
            "NIP-07 nip44 encrypt: wasm target required".to_string(),
        ))
    }
    fn decrypt(&self, _sender: &PublicKey, _payload: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported(
            "NIP-07 nip44 decrypt: wasm target required".to_string(),
        ))
    }
}
