//! NIP-07 (browser extension) signer.
//!
//! On non-wasm builds the signer is a structural stub: `pubkey()` returns the
//! cached key the constructor was handed and every other operation returns
//! [`SignerError::Unsupported`]. This keeps the trait shape available
//! everywhere the workspace compiles (Swift FFI integration tests, native
//! conformance harnesses) without pretending wasm-only capabilities exist.
//!
//! On wasm32 + `feature = "wasm"` builds, [`Nip07Signer::sign`] reaches into
//! the JS event loop through `wasm-bindgen-futures::spawn_local` and calls
//! `window.nostr.signEvent(...)`. The returned Promise is awaited off-thread
//! and the resolved signed event is pushed back through an
//! `std::sync::mpsc::Receiver` the [`SignerOp::Pending`] carries to the
//! caller. The caller still drives the op synchronously via `poll()` /
//! `wait()` — this is what lets the actor loop integrate the signer without
//! pulling in tokio (see `nmp-signer-iface::op` for the contract).
//!
//! NIP-04 / NIP-44 namespaces are still `Unsupported` on every build —
//! adding `window.nostr.nip04.*` / `nip44.*` bridges is a follow-up; the
//! Stage 3b scope is event signing only.
//!
//! D6 (no panics across the public surface): `pubkey()` cannot fail because
//! construction is gated on a cached pubkey (see `from_payload`); every
//! other failure mode is a structured [`SignerError`] the caller maps to
//! `toast: Option<String>` at the FFI boundary.

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::PublicKey;

use super::payload::{Nip07Payload, SignerPayload};
use super::traits::{Nip04, Nip44, Signer, SignerBackend, SignerError};
use super::SignerOp;

/// Browser-extension NIP-07 signer.
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

    /// # Wasm hazard: never call `SignerOp::wait()` on the returned value
    ///
    /// On wasm32 the returned `SignerOp::Pending(rx)` only resolves when the
    /// JS event loop runs the `spawn_local` future that awaits
    /// `window.nostr.signEvent(...).then(...)`. `SignerOp::wait()` calls
    /// `recv_timeout`, which blocks the wasm thread — and since wasm32 runs
    /// the JS event loop on the same thread, blocking it prevents the
    /// future from ever resolving. The result: deadlock until the
    /// `recv_timeout` returns `Timeout`.
    ///
    /// On wasm32, callers MUST poll (e.g. yield to JS via another
    /// `spawn_local`, then re-poll). The future-driven publish path Stage 3c
    /// will introduce wraps this hazard inside an `async fn`, so application
    /// code never sees it directly.
    fn sign(&self, unsigned: UnsignedEvent) -> SignerOp<SignedEvent> {
        sign_impl(&self.cached_pubkey, unsigned)
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

// ─── sign() backends ─────────────────────────────────────────────────────────
//
// Two compile-time selected implementations:
//
// 1. Non-wasm path (every native target, plus wasm32 builds compiled WITHOUT
//    the `wasm` Cargo feature): no extension to talk to. Returns
//    `Unsupported` synchronously. This is the path the `nmp-testing` fixture
//    and the iOS/macOS/Linux conformance harnesses see.
//
// 2. wasm32 + `feature = "wasm"` path: dispatches to `window.nostr.signEvent`
//    through `wasm-bindgen-futures::spawn_local`. Returns
//    `SignerOp::Pending(rx)` immediately; the actual sign roundtrip completes
//    on the JS event loop, and the caller polls the receiver. No tokio.

#[cfg(not(all(target_arch = "wasm32", feature = "wasm")))]
fn sign_impl(_cached_pubkey: &PublicKey, _unsigned: UnsignedEvent) -> SignerOp<SignedEvent> {
    SignerOp::err(SignerError::Unsupported(
        "NIP-07 signing requires wasm target + browser extension; \
         enable feature = \"wasm\" and target wasm32-unknown-unknown"
            .to_string(),
    ))
}

#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
fn sign_impl(cached_pubkey: &PublicKey, unsigned: UnsignedEvent) -> SignerOp<SignedEvent> {
    wasm::sign_with_extension(*cached_pubkey, unsigned)
}

// V-01 Stage 3c — the `wasm` submodule (window.nostr.signEvent bridge +
// async twin) is extracted to a sibling file so this module stays under the
// AGENTS.md 500-LOC ceiling. `#[path = "nip07/wasm.rs"]` preserves the
// `crate::signers::nip07::wasm` module path so every existing re-export
// (`pub use nip07::wasm::*`) resolves byte-identically.
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
#[path = "nip07/wasm.rs"]
pub mod wasm;

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
