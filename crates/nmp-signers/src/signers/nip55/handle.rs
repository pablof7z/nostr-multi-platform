//! `RemoteSignerHandle` impl for `Nip55Signer`.
//!
//! This is the kernel-facing adapter for the NIP-55 path.  The actor only ever
//! holds `Box<dyn RemoteSignerHandle>` — keeping doctrine **D0** intact
//! (`nmp-core` does not import `nmp-signers`).
//!
//! ## Responsibility split
//!
//! - `sign` delegates to `Nip55Signer::enqueue(SignEvent, …)` → mapper.
//! - `nip44_encrypt` / `nip44_decrypt` delegate to `Nip44 for Nip55Signer`.
//! - `deliver_response` deserialises `ExternalSignerResponse` and resolves
//!   the pending correlation id via `Nip55Signer::deliver_external_response`.
//! - `op_timeout` overrides the default 5s with `EXTERNAL_SIGN_TIMEOUT`
//!   = 90s (ADR-0048 D3 / ADR-0050 D4 — Intent round-trip requires user
//!   interaction; the one budget now covers sign + nip44 verbs).
//! - `disconnect` drains the pending table with an error so blocked ops fail
//!   fast instead of waiting out the 90s deadline.
//!
//! Per **D6** this file never `unwrap()`s or panics on malformed input — bad
//! JSON in `deliver_response` is silently dropped (degrades to timeout).

use std::time::Duration;

use nmp_signer_iface::RemoteSignerHandle;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};
use nmp_signer_iface::{SignerError, SignerOp};
use nostr::PublicKey;

use super::Nip55Signer;
use crate::signers::traits::{Nip44, Signer};
use nmp_signer_iface::EXTERNAL_SIGN_TIMEOUT;

impl RemoteSignerHandle for Nip55Signer {
    fn pubkey_hex(&self) -> String {
        self.user_pubkey().to_hex()
    }

    fn signer_kind(&self) -> &'static str {
        "nip55"
    }

    fn persistence_payload_json(&self) -> Option<String> {
        self.to_payload()
            .ok()
            .and_then(|p| serde_json::to_string(&p).ok())
    }

    /// Per-op deadline for NIP-55 signer operations (sign + nip44 verbs).
    ///
    /// 90s — an Android Intent round-trip requires the user to foreground Amber
    /// and tap approve. ADR-0048 D3 / ADR-0050 D4; overrides the 5s NIP-46
    /// default.
    fn op_timeout(&self) -> Duration {
        EXTERNAL_SIGN_TIMEOUT
    }

    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        <Self as Signer>::sign(self, unsigned.clone())
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        let recipient = match PublicKey::from_hex(recipient_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                return SignerOp::err(SignerError::Backend(format!(
                    "nip55 invalid recipient pubkey: {e}"
                )))
            }
        };
        <Self as Nip44>::encrypt(self, &recipient, plaintext)
    }

    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String> {
        let sender = match PublicKey::from_hex(sender_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                return SignerOp::err(SignerError::Backend(format!(
                    "nip55 invalid sender pubkey: {e}"
                )))
            }
        };
        <Self as Nip44>::decrypt(self, &sender, ciphertext)
    }

    fn deliver_response(&self, response_json: &str) {
        self.deliver_external_response(response_json);
    }

    fn disconnect(&self) {
        self.drain_pending_with_error("nip55 signer disconnected");
    }
}
