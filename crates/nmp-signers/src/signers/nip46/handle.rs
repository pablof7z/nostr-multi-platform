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
//! - `deliver_response` is the inbound RPC hook: it delegates decoded
//!   response ingestion to `Nip46Signer::ingest_rpc_response`.
//!
//! Per **D6** (no panics across FFI), this file never `unwrap()`s or panics on
//! malformed input — bad JSON is logged and dropped.

use nmp_signer_iface::RemoteSignerHandle;
use nmp_signer_iface::{
    Nip44DecryptBatchRequest, Nip44DecryptBatchResult, Nip44DecryptSessionBeginRequest,
    Nip44DecryptSessionEndRequest, Nip44DecryptSessionExtension, Nip44DecryptSessionGrant,
    SignedEvent, UnsignedEvent, NMP_NIP44_DECRYPT_BATCH, NMP_NIP44_DECRYPT_SESSION_BEGIN,
    NMP_NIP44_DECRYPT_SESSION_END,
};
use nmp_signer_iface::{SignerError, SignerOp};
use nostr::PublicKey;

use super::result_map::{map_response_with_error, parse_json_result};
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
        self.to_payload()
            .ok()
            .and_then(|p| serde_json::to_string(&p).ok())
    }

    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        <Self as Signer>::sign(self, unsigned.clone())
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        // ADR-0072: the actor-facing trait carries hex; parse it here before
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
                return SignerOp::err(SignerError::Backend(format!("invalid sender pubkey: {e}")))
            }
        };
        <Self as Nip44>::decrypt(self, &sender, ciphertext)
    }

    fn nip44_decrypt_session_begin(
        &self,
        request: Nip44DecryptSessionBeginRequest,
    ) -> SignerOp<Nip44DecryptSessionGrant> {
        let params_json = match params_json(&request) {
            Ok(s) => s,
            Err(e) => return SignerOp::err(e),
        };
        let extension = self.decrypt_session_extension.clone();
        map_response_with_error(
            self.enqueue(NMP_NIP44_DECRYPT_SESSION_BEGIN, &params_json),
            move |result| {
                let grant = parse_json_result::<Nip44DecryptSessionGrant>(
                    &result,
                    NMP_NIP44_DECRYPT_SESSION_BEGIN,
                )?;
                if let Ok(mut slot) = extension.lock() {
                    *slot = Some(Nip44DecryptSessionExtension::default());
                }
                Ok(grant)
            },
            |error| classify_extension_error(error, NMP_NIP44_DECRYPT_SESSION_BEGIN),
        )
    }

    fn nip44_decrypt_batch(
        &self,
        request: Nip44DecryptBatchRequest,
    ) -> SignerOp<Nip44DecryptBatchResult> {
        let params_json = match params_json(&request) {
            Ok(s) => s,
            Err(e) => return SignerOp::err(e),
        };
        map_response_with_error(
            self.enqueue(NMP_NIP44_DECRYPT_BATCH, &params_json),
            |result| parse_json_result(&result, NMP_NIP44_DECRYPT_BATCH),
            |error| classify_extension_error(error, NMP_NIP44_DECRYPT_BATCH),
        )
    }

    fn nip44_decrypt_session_end(&self, request: Nip44DecryptSessionEndRequest) -> SignerOp<bool> {
        let params_json = match params_json(&request) {
            Ok(s) => s,
            Err(e) => return SignerOp::err(e),
        };
        map_response_with_error(
            self.enqueue(NMP_NIP44_DECRYPT_SESSION_END, &params_json),
            |result| parse_json_result(&result, NMP_NIP44_DECRYPT_SESSION_END),
            |error| classify_extension_error(error, NMP_NIP44_DECRYPT_SESSION_END),
        )
    }

    fn deliver_response(&self, response_json: &str) {
        self.ingest_rpc_response(response_json);
    }

    fn disconnect(&self) {
        self.drain_pending_with_error("signer disconnected");
    }
}

fn params_json<T: serde::Serialize>(request: &T) -> Result<String, SignerError> {
    serde_json::to_string(request)
        .map(|payload| format!("[{payload}]"))
        .map_err(|e| SignerError::Backend(format!("serialize nip44 decrypt-session request: {e}")))
}

fn classify_extension_error(error: SignerError, method: &str) -> SignerError {
    let SignerError::Rejected(message) = error else {
        return error;
    };
    let lower = message.to_ascii_lowercase();
    let method_lower = method.to_ascii_lowercase();
    let is_unknown_method = lower.contains("unknown method")
        || lower.contains("method not found")
        || lower.contains("no such method")
        || lower.contains("not implemented")
        || (lower.contains("unsupported") && lower.contains(&method_lower));
    if is_unknown_method {
        SignerError::Unsupported(message)
    } else {
        SignerError::Rejected(message)
    }
}

#[cfg(test)]
#[path = "handle/decrypt_session_tests.rs"]
mod decrypt_session_tests;

#[cfg(test)]
#[path = "handle/debug_tests.rs"]
mod debug_tests;

#[cfg(test)]
#[path = "handle/tests.rs"]
mod tests;
