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

use super::Nip46Signer;
use crate::signers::traits::Signer;

impl RemoteSignerHandle for Nip46Signer {
    fn pubkey_hex(&self) -> String {
        self.remote_user_pubkey().to_hex()
    }

    fn signer_kind(&self) -> &'static str {
        "nip46"
    }

    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        <Self as Signer>::sign(self, unsigned.clone())
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
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| err_val.to_string());
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
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use nmp_core::substrate::UnsignedEvent;
    use nmp_core::RemoteSignerHandle;
    use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError};

    use crate::signers::traits::Signer;
    use crate::{LocalKeySigner, Nip46SignerHandle};

    const SAMPLE_PK: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

    #[derive(Debug, Default)]
    struct StubTransport {
        sent: Mutex<Vec<Nip46Rpc>>,
    }

    impl Nip46Transport for StubTransport {
        fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
            self.sent.lock().unwrap().push(rpc);
            Ok(())
        }
    }

    fn build_signer_with_remote(
        remote_user: &LocalKeySigner,
    ) -> (super::Nip46Signer, Arc<StubTransport>) {
        let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com&secret=s1");
        let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
        let transport = Arc::new(StubTransport::default());
        let signer = handle.complete(transport.clone(), remote_user.pubkey());
        (signer, transport)
    }

    #[test]
    fn pubkey_hex_returns_remote_user() {
        let remote_user = LocalKeySigner::generate();
        let (signer, _t) = build_signer_with_remote(&remote_user);
        assert_eq!(
            RemoteSignerHandle::pubkey_hex(&signer),
            remote_user.pubkey().to_hex(),
        );
    }

    #[test]
    fn signer_kind_is_nip46() {
        let remote_user = LocalKeySigner::generate();
        let (signer, _t) = build_signer_with_remote(&remote_user);
        assert_eq!(RemoteSignerHandle::signer_kind(&signer), "nip46");
    }

    #[test]
    fn deliver_rpc_response_resolves_pending_sign() {
        // Round-trip: start a sign() (Pending), feed back a real signed event
        // via deliver_rpc_response, observe the mapper-validated output.
        let remote_user = LocalKeySigner::generate();
        let remote_pubkey = remote_user.pubkey();
        let (signer, transport) = build_signer_with_remote(&remote_user);

        let unsigned = UnsignedEvent {
            pubkey: remote_pubkey.to_hex(),
            kind: 1,
            tags: vec![],
            content: "hello bunker".to_string(),
            created_at: 1_700_000_000,
        };

        // Drive sign() via the trait-method-under-test (RemoteSignerHandle::sign)
        // so the test covers the adapter path, not just the inner Signer impl.
        let op = RemoteSignerHandle::sign(&signer, &unsigned);

        // Inspect the queued RPC to learn its id.
        let sent = transport.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 1);
        let rpc_id = sent[0].id.clone();

        // Produce a real signed event for the response body — the mapper runs
        // nostr::Event::verify(), so the payload must be cryptographically valid.
        let real_signed = <LocalKeySigner as Signer>::sign(&remote_user, unsigned.clone())
            .wait(Duration::from_secs(1))
            .expect("real sign");
        let result_body = format!(
            r#"{{"id":"{}","pubkey":"{}","sig":"{}","kind":{},"created_at":{},"tags":[],"content":"{}"}}"#,
            real_signed.id,
            real_signed.unsigned.pubkey,
            real_signed.sig,
            real_signed.unsigned.kind,
            real_signed.unsigned.created_at,
            real_signed.unsigned.content,
        );
        // NIP-46 envelope: {"id": "<req-id>", "result": "<signed-event-json>"}
        let envelope = serde_json::json!({
            "id": rpc_id,
            "result": result_body,
        })
        .to_string();
        RemoteSignerHandle::deliver_rpc_response(&signer, &envelope);

        let signed = op
            .wait(Duration::from_secs(2))
            .expect("signed event arrives");
        assert_eq!(signed.id, real_signed.id);
        assert_eq!(signed.sig, real_signed.sig);
        assert_eq!(signed.unsigned.pubkey, remote_pubkey.to_hex());
    }

    #[test]
    fn deliver_rpc_response_with_error_field_routes_rejected() {
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);

        let unsigned = UnsignedEvent {
            pubkey: remote_user.pubkey().to_hex(),
            kind: 1,
            tags: vec![],
            content: "denied".to_string(),
            created_at: 1_700_000_000,
        };
        let op = RemoteSignerHandle::sign(&signer, &unsigned);
        let rpc_id = transport.sent.lock().unwrap()[0].id.clone();

        let envelope = serde_json::json!({
            "id": rpc_id,
            "error": "user denied",
        })
        .to_string();
        RemoteSignerHandle::deliver_rpc_response(&signer, &envelope);

        let err = op
            .wait(Duration::from_secs(2))
            .expect_err("error envelope must surface as Err");
        match err {
            SignerError::Rejected(m) => assert!(m.contains("user denied")),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn disconnect_drains_pending_immediately() {
        // A `sign()` in flight leaves a Pending one-shot in `pending`.
        // disconnect() must resolve it with Err(Rejected) at once so the
        // SignerOp::wait caller fails fast instead of hanging for the timeout.
        let remote_user = LocalKeySigner::generate();
        let (signer, _transport) = build_signer_with_remote(&remote_user);

        let unsigned = UnsignedEvent {
            pubkey: remote_user.pubkey().to_hex(),
            kind: 1,
            tags: vec![],
            content: "in flight".to_string(),
            created_at: 1_700_000_000,
        };
        let op = RemoteSignerHandle::sign(&signer, &unsigned);

        // End the session: every pending request resolves immediately.
        RemoteSignerHandle::disconnect(&signer);

        let err = op
            .wait(Duration::from_millis(100))
            .expect_err("disconnect must surface as Err, not a timeout");
        match err {
            SignerError::Rejected(m) => assert!(m.contains("disconnected")),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn deliver_rpc_response_with_invalid_json_is_dropped() {
        // D6: invalid JSON must not panic.  We also assert that a subsequent
        // valid envelope still resolves — the signer is not poisoned.
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);

        let unsigned = UnsignedEvent {
            pubkey: remote_user.pubkey().to_hex(),
            kind: 1,
            tags: vec![],
            content: "robust".to_string(),
            created_at: 1_700_000_000,
        };
        let op = RemoteSignerHandle::sign(&signer, &unsigned);
        let rpc_id = transport.sent.lock().unwrap()[0].id.clone();

        // Garbage in — silent drop.
        RemoteSignerHandle::deliver_rpc_response(&signer, "not json {{");
        // Missing id — silent drop.
        RemoteSignerHandle::deliver_rpc_response(&signer, r#"{"result":"x"}"#);

        // Now a real error envelope must still land.
        let envelope = serde_json::json!({
            "id": rpc_id,
            "error": "later",
        })
        .to_string();
        RemoteSignerHandle::deliver_rpc_response(&signer, &envelope);

        let err = op
            .wait(Duration::from_secs(2))
            .expect_err("error envelope must surface");
        assert!(matches!(err, SignerError::Rejected(_)));
    }
}
