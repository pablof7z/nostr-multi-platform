//! NIP-07 (browser extension) signer.
//!
//! Real `window.nostr.*` bindings require a wasm target which is not yet wired
//! into this workspace.  This module ships the trait shape + serialization
//! payload so the wasm follow-up is a pure additive change (no API churn).
//!
//! Non-wasm builds keep the payload + trait shape available.  Operations return
//! `Unsupported`; `pubkey()` requires a cached pubkey from a prior payload.

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::PublicKey;

use super::payload::{Nip07Payload, SignerPayload};
use super::traits::{Nip04, Nip44, Signer, SignerBackend, SignerError};
use super::SignerOp;

/// Browser-extension NIP-07 signer.
///
/// On non-wasm builds, every operation returns
/// [`SignerError::Unsupported`] with a clear "wasm target required" message.
/// The cached pubkey, if any, is still accessible — apps can render the
/// avatar/handle from a previous session without an extension call.
#[derive(Debug)]
pub struct Nip07Signer {
    cached_pubkey: Option<PublicKey>,
}

impl Nip07Signer {
    /// Construct an empty NIP-07 signer (no cached pubkey).  This value cannot
    /// satisfy `pubkey()` until a wasm handshake or cached payload supplies one.
    pub fn new() -> Self {
        Self { cached_pubkey: None }
    }

    /// Restore from a payload, optionally re-using a cached pubkey.
    pub fn from_payload(p: &Nip07Payload) -> Result<Self, SignerError> {
        let cached = p
            .cached_pubkey_hex
            .as_deref()
            .map(PublicKey::from_hex)
            .transpose()
            .map_err(|e| SignerError::Backend(format!("invalid cached nip07 pubkey: {e}")))?;
        Ok(Self {
            cached_pubkey: cached,
        })
    }

    /// Whether the current build can actually talk to the extension.
    pub const fn nip07_supported() -> bool {
        cfg!(all(target_arch = "wasm32", feature = "wasm"))
    }
}

impl Default for Nip07Signer {
    fn default() -> Self {
        Self::new()
    }
}

impl Signer for Nip07Signer {
    fn backend(&self) -> SignerBackend {
        SignerBackend::Nip07
    }

    fn pubkey(&self) -> PublicKey {
        // Non-wasm: must have been restored from a cached payload.  Returning
        // a zero pubkey would silently corrupt downstream logic; we panic with
        // a descriptive message so the bug is loud during development.
        //
        // On wasm-with-feature: a future revision calls
        // `web_sys::window().nostr().get_public_key().await` synchronously via
        // the cached value (populated by `Nip07Signer::handshake().await`).
        self.cached_pubkey.unwrap_or_else(|| {
            panic!(
                "Nip07Signer::pubkey() called without cached pubkey; \
                 wasm target + handshake required (see ADR-0015)"
            )
        })
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
            cached_pubkey_hex: self.cached_pubkey.map(|p| p.to_hex()),
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
