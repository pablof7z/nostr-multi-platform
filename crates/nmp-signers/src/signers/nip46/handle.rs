//! `RemoteSignerHandle` impl for `Nip46Signer`.
//!
//! This is the kernel-facing adapter declared in `nmp-core::remote_signer`.
//! The actor only ever holds `Box<dyn RemoteSignerHandle>` — keeping doctrine
//! **D0** intact (`nmp-core` does not import `nmp-signers`).
//!
//! ## Responsibility split
//!
//! - `sign` delegates to the existing `Signer::sign` impl, which already
//!   returns `SignerOp<SignedEvent>` with mapper-validated responses.
//! - `deliver_rpc_response` is the inbound RPC hook: it parses the decoded
//!   `{"id":"...","result":"..."}` (or `{"error":"..."}`) envelope and routes
//!   the resolution to the pending one-shot channel via `resolve_response`.
//!
//! Per **D6** (no panics across FFI), this file never `unwrap()`s or panics on
//! malformed input — bad JSON is logged and dropped.

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nmp_core::RemoteSignerHandle;
use nmp_signer_iface::{SignerError, SignerOp};
use nostr::PublicKey;

use super::Nip46Signer;
use crate::signers::traits::{Nip44, Signer};

impl RemoteSignerHandle for Nip46Signer {
    fn pubkey_hex(&self) -> String {
        self.remote_user_pubkey().to_hex()
    }

    fn signer_kind(&self) -> &'static str {
        "nip46"
    }

    fn persistence_payload_json(&self) -> Option<String> {
        serde_json::to_string(&self.to_payload()).ok()
    }

    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        <Self as Signer>::sign(self, unsigned.clone())
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        // ADR-0026: the actor-facing trait carries hex; parse it here before
        // delegating to the existing `Nip44` impl. A malformed pubkey surfaces
        // as a `SignerOp` error (D6 — never a panic across the seam).
        let recipient = match PublicKey::from_hex(recipient_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                return SignerOp::err(SignerError::Backend(format!(
                    "invalid recipient pubkey: {e}"
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
                    "invalid sender pubkey: {e}"
                )))
            }
        };
        <Self as Nip44>::decrypt(self, &sender, ciphertext)
    }

    fn deliver_rpc_response(&self, response_json: &str) {
        // D6: no panics or stray stdio across FFI. Malformed input is dropped
        // silently — the originating `sign()` SignerOp times out on its own, so
        // a dropped envelope degrades gracefully without an `eprintln!` in
        // library code. `nmp-signers` carries no `tracing` dep by design.
        let Ok(v) = serde_json::from_str::<serde_json::Value>(response_json) else {
            return;
        };

        let Some(id) = v.get("id").and_then(|x| x.as_str()) else {
            return;
        };

        // Prefer an explicit non-null `error` over `result`.  Some bunkers
        // include both fields (`error` set, `result` empty) per NIP-46.
        if let Some(err_val) = v.get("error") {
            if !err_val.is_null() {
                let msg = err_val
                    .as_str().map_or_else(|| err_val.to_string(), str::to_string);
                self.resolve_response(id, Err(SignerError::Rejected(msg)));
                return;
            }
        }

        // No usable `result` — drop and let the pending `sign()` time out.
        if let Some(result) = v.get("result").and_then(|x| x.as_str()) {
            self.resolve_response(id, Ok(result.to_string()));
        }
    }

    fn disconnect(&self) {
        self.drain_pending_with_error("signer disconnected");
    }
}

#[cfg(test)]
#[path = "handle/tests.rs"]
mod tests;
