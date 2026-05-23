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
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use nmp_core::substrate::UnsignedEvent;
    use nmp_core::RemoteSignerHandle;
    use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError};

    use crate::signers::payload::SignerPayload;
    use crate::signers::traits::{Nip04, Nip44, Signer};
    use crate::{LocalKeySigner, Nip46Signer, Nip46SignerHandle};

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

    /// A transport whose `send_rpc` always fails — exercises `enqueue`'s
    /// transmit-failure branch.
    #[derive(Debug, Default)]
    struct FailingTransport;

    impl Nip46Transport for FailingTransport {
        fn send_rpc(&self, _rpc: Nip46Rpc) -> Result<(), SignerError> {
            Err(SignerError::Backend("relay pool offline".to_string()))
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

    #[test]
    fn deliver_rpc_response_prefers_result_when_error_is_null() {
        // Some bunkers always include both fields; an explicit `error: null`
        // means "no error" — the `result` must win.  This pins the null-error
        // branch of `deliver_rpc_response`.
        let remote_user = LocalKeySigner::generate();
        let remote_pubkey = remote_user.pubkey();
        let (signer, transport) = build_signer_with_remote(&remote_user);

        let unsigned = UnsignedEvent {
            pubkey: remote_pubkey.to_hex(),
            kind: 1,
            tags: vec![],
            content: "null error".to_string(),
            created_at: 1_700_000_000,
        };
        let op = RemoteSignerHandle::sign(&signer, &unsigned);
        let rpc_id = transport.sent.lock().unwrap()[0].id.clone();

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
        // Envelope carries BOTH `error: null` and a real `result`.
        let envelope = serde_json::json!({
            "id": rpc_id,
            "error": serde_json::Value::Null,
            "result": result_body,
        })
        .to_string();
        RemoteSignerHandle::deliver_rpc_response(&signer, &envelope);

        let signed = op
            .wait(Duration::from_secs(2))
            .expect("null error must not block the result");
        assert_eq!(signed.id, real_signed.id);
    }

    #[test]
    fn deliver_rpc_response_with_unknown_id_is_dropped() {
        // A response addressed to an id we never registered must be a silent
        // no-op — no panic, and the genuinely-pending op stays pending.
        let remote_user = LocalKeySigner::generate();
        let (signer, _transport) = build_signer_with_remote(&remote_user);

        let unsigned = UnsignedEvent {
            pubkey: remote_user.pubkey().to_hex(),
            kind: 1,
            tags: vec![],
            content: "orphan".to_string(),
            created_at: 1_700_000_000,
        };
        let mut op = RemoteSignerHandle::sign(&signer, &unsigned);

        let envelope = serde_json::json!({
            "id": "an-id-we-never-issued",
            "result": "whatever",
        })
        .to_string();
        RemoteSignerHandle::deliver_rpc_response(&signer, &envelope);

        // The real op must still be pending — the stray response did not
        // resolve it.
        assert!(op.poll().is_none(), "unknown-id response must not resolve a pending op");
    }

    // ---- enqueue / transport failure --------------------------------------

    #[test]
    fn sign_when_transport_send_fails_surfaces_err_not_panic() {
        // `enqueue` registers the pending entry, then calls `send_rpc`.  When
        // the transport rejects, the failure must surface as `SignerOp::err`
        // (D6: an error, never a panic or a hang) AND the pending entry must
        // be cleaned up — a failed send produces no response, so a retained
        // entry leaks for the signer's lifetime.
        let remote_user = LocalKeySigner::generate();
        let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com");
        let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
        let signer = handle.complete(Arc::new(FailingTransport), remote_user.pubkey());

        let unsigned = UnsignedEvent {
            pubkey: remote_user.pubkey().to_hex(),
            kind: 1,
            tags: vec![],
            content: "doomed".to_string(),
            created_at: 1_700_000_000,
        };
        let op = RemoteSignerHandle::sign(&signer, &unsigned);
        match op.wait(Duration::from_millis(100)) {
            Err(SignerError::Backend(m)) => assert!(m.contains("relay pool offline")),
            other => panic!("expected Backend Err from failed send, got {other:?}"),
        }
        assert_eq!(
            signer.pending_len(),
            0,
            "a failed send must not leak a pending RPC entry"
        );
    }

    #[test]
    fn repeated_failed_sends_do_not_accumulate_pending_entries() {
        // Regression guard for the orphan-entry leak: even after many failed
        // sends the pending map stays empty.
        let remote_user = LocalKeySigner::generate();
        let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com");
        let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
        let signer = handle.complete(Arc::new(FailingTransport), remote_user.pubkey());

        let unsigned = UnsignedEvent {
            pubkey: remote_user.pubkey().to_hex(),
            kind: 1,
            tags: vec![],
            content: "doomed".to_string(),
            created_at: 1_700_000_000,
        };
        for _ in 0..16 {
            let _ = <super::Nip46Signer as Signer>::sign(&signer, unsigned.clone());
        }
        assert_eq!(
            signer.pending_len(),
            0,
            "16 failed sends must not accumulate 16 orphan entries"
        );
    }

    #[test]
    fn nip04_encrypt_when_transport_send_fails_surfaces_err() {
        let remote_user = LocalKeySigner::generate();
        let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com");
        let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
        let signer = handle.complete(Arc::new(FailingTransport), remote_user.pubkey());

        let recipient = LocalKeySigner::generate().pubkey();
        let op = Nip04::encrypt(&signer, &recipient, "secret message");
        match op.wait(Duration::from_millis(100)) {
            Err(SignerError::Backend(_)) => {}
            other => panic!("expected Backend Err, got {other:?}"),
        }
        assert_eq!(
            signer.pending_len(),
            0,
            "a failed nip04_encrypt send must not leak a pending entry"
        );
    }

    // ---- Nip04 / Nip44 RPC enqueue shape ----------------------------------

    /// Drain the single queued RPC, asserting exactly one was sent.
    fn single_rpc(transport: &StubTransport) -> Nip46Rpc {
        let sent = transport.sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "expected exactly one queued RPC");
        sent[0].clone()
    }

    #[test]
    fn nip04_encrypt_queues_rpc_with_correct_method_and_params() {
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);
        let recipient = LocalKeySigner::generate().pubkey();

        let _op = Nip04::encrypt(&signer, &recipient, "hello \"world\"");
        let rpc = single_rpc(&transport);
        assert!(rpc.body_json.contains(r#""method":"nip04_encrypt""#));
        assert!(rpc.body_json.contains(&recipient.to_hex()));
        // The plaintext's embedded quote must be JSON-escaped in the params.
        assert!(rpc.body_json.contains(r#"hello \"world\""#));
        assert_eq!(rpc.remote_pubkey_hex, SAMPLE_PK);
        assert_eq!(rpc.relays, vec!["wss://relay.example.com".to_string()]);
    }

    #[test]
    fn nip04_decrypt_queues_rpc_with_correct_method() {
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);
        let sender = LocalKeySigner::generate().pubkey();

        let _op = Nip04::decrypt(&signer, &sender, "ciphertext?iv=abc");
        let rpc = single_rpc(&transport);
        assert!(rpc.body_json.contains(r#""method":"nip04_decrypt""#));
        assert!(rpc.body_json.contains(&sender.to_hex()));
    }

    #[test]
    fn nip44_encrypt_queues_rpc_with_correct_method() {
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);
        let recipient = LocalKeySigner::generate().pubkey();

        let _op = Nip44::encrypt(&signer, &recipient, "nip44 plaintext");
        let rpc = single_rpc(&transport);
        assert!(rpc.body_json.contains(r#""method":"nip44_encrypt""#));
        assert!(rpc.body_json.contains(&recipient.to_hex()));
    }

    #[test]
    fn nip44_decrypt_queues_rpc_with_correct_method() {
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);
        let sender = LocalKeySigner::generate().pubkey();

        let _op = Nip44::decrypt(&signer, &sender, "nip44-payload");
        let rpc = single_rpc(&transport);
        assert!(rpc.body_json.contains(r#""method":"nip44_decrypt""#));
        assert!(rpc.body_json.contains(&sender.to_hex()));
    }

    #[test]
    fn nip04_encrypt_round_trips_via_resolve_response() {
        // The encrypt RPC resolves to an opaque ciphertext string — unlike
        // sign_event there is no verify() step, so the raw `result` is the
        // value the caller receives.
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);
        let recipient = LocalKeySigner::generate().pubkey();

        let op = Nip04::encrypt(&signer, &recipient, "plaintext");
        let rpc_id = single_rpc(&transport).id;

        signer.resolve_response(&rpc_id, Ok("ciphertext-blob".to_string()));
        let got = op.wait(Duration::from_secs(1)).expect("encrypt resolves");
        assert_eq!(got, "ciphertext-blob");
    }

    #[test]
    fn nip44_decrypt_error_response_surfaces_as_err() {
        // An RPC error for a decrypt op must surface as Err — never panic.
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);
        let sender = LocalKeySigner::generate().pubkey();

        let op = Nip44::decrypt(&signer, &sender, "bad-payload");
        let rpc_id = single_rpc(&transport).id;

        signer.resolve_response(
            &rpc_id,
            Err(SignerError::Rejected("cannot decrypt".to_string())),
        );
        match op.wait(Duration::from_secs(1)) {
            Err(SignerError::Rejected(m)) => assert!(m.contains("cannot decrypt")),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    // ---- RemoteSignerHandle NIP-44 seam (ADR-0026) ------------------------

    #[test]
    fn remote_handle_nip44_encrypt_queues_rpc_and_round_trips() {
        // ADR-0026: the actor-facing `RemoteSignerHandle::nip44_encrypt` parses
        // hex, then delegates to the inner `Nip44` impl — the RPC must carry
        // the `nip44_encrypt` method and the opaque ciphertext result must
        // surface verbatim (no verify() step).
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);
        let recipient = LocalKeySigner::generate().pubkey();

        let op =
            RemoteSignerHandle::nip44_encrypt(&signer, &recipient.to_hex(), "seal plaintext");
        let rpc = single_rpc(&transport);
        assert!(rpc.body_json.contains(r#""method":"nip44_encrypt""#));
        assert!(rpc.body_json.contains(&recipient.to_hex()));

        signer.resolve_response(&rpc.id, Ok("sealed-ciphertext".to_string()));
        let got = op.wait(Duration::from_secs(1)).expect("encrypt resolves");
        assert_eq!(got, "sealed-ciphertext");
    }

    #[test]
    fn remote_handle_nip44_decrypt_queues_rpc_with_sender() {
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);
        let sender = LocalKeySigner::generate().pubkey();

        let _op =
            RemoteSignerHandle::nip44_decrypt(&signer, &sender.to_hex(), "sealed-payload");
        let rpc = single_rpc(&transport);
        assert!(rpc.body_json.contains(r#""method":"nip44_decrypt""#));
        assert!(rpc.body_json.contains(&sender.to_hex()));
    }

    #[test]
    fn remote_handle_nip44_encrypt_with_malformed_pubkey_surfaces_err() {
        // D6: a bad hex pubkey must surface as a SignerOp error, never panic,
        // and must NOT enqueue an RPC.
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);

        let op = RemoteSignerHandle::nip44_encrypt(&signer, "not-hex", "plaintext");
        match op.wait(Duration::from_millis(100)) {
            Err(SignerError::Backend(m)) => assert!(m.contains("invalid recipient pubkey")),
            other => panic!("expected Backend Err, got {other:?}"),
        }
        assert_eq!(
            transport.sent.lock().unwrap().len(),
            0,
            "a malformed pubkey must not enqueue an RPC"
        );
    }

    #[test]
    fn nip46_signer_exposes_nip04_and_nip44_namespaces() {
        // Per the Signer trait contract, a NIP-46 signer advertises both
        // encryption namespaces (the remote bunker services them).
        let remote_user = LocalKeySigner::generate();
        let (signer, _t) = build_signer_with_remote(&remote_user);
        assert!(Signer::nip04(&signer).is_some());
        assert!(Signer::nip44(&signer).is_some());
    }

    // ---- from_payload failure paths ---------------------------------------

    #[test]
    fn from_payload_without_cached_pubkey_returns_not_ready() {
        // A payload that has never completed a handshake has no cached remote
        // pubkey — restore must refuse with NotReady, not panic.
        let remote_user = LocalKeySigner::generate();
        let (signer, _t) = build_signer_with_remote(&remote_user);
        let SignerPayload::Nip46(mut payload) = signer.to_payload() else {
            panic!("expected nip46 payload");
        };
        payload.cached_remote_user_pubkey_hex = None;

        let err = Nip46Signer::from_payload(&payload, Arc::new(StubTransport::default()))
            .expect_err("payload without cached pubkey must be refused");
        match err {
            SignerError::NotReady(m) => assert!(m.contains("cached remote user pubkey")),
            other => panic!("expected NotReady, got {other:?}"),
        }
    }

    #[test]
    fn from_payload_with_invalid_cached_pubkey_returns_backend_err() {
        let remote_user = LocalKeySigner::generate();
        let (signer, _t) = build_signer_with_remote(&remote_user);
        let SignerPayload::Nip46(mut payload) = signer.to_payload() else {
            panic!("expected nip46 payload");
        };
        payload.cached_remote_user_pubkey_hex = Some("not-valid-hex".to_string());

        let err = Nip46Signer::from_payload(&payload, Arc::new(StubTransport::default()))
            .expect_err("garbage cached pubkey must be refused");
        assert!(
            matches!(err, SignerError::Backend(m) if m.contains("cached remote pubkey")),
            "expected Backend(cached remote pubkey)"
        );
    }

    #[test]
    fn from_payload_with_invalid_local_secret_returns_backend_err() {
        let remote_user = LocalKeySigner::generate();
        let (signer, _t) = build_signer_with_remote(&remote_user);
        let SignerPayload::Nip46(mut payload) = signer.to_payload() else {
            panic!("expected nip46 payload");
        };
        payload.local_secret_hex =
            zeroize::Zeroizing::new("zzzz-not-a-secret".to_string());

        let err = Nip46Signer::from_payload(&payload, Arc::new(StubTransport::default()))
            .expect_err("garbage local secret must be refused");
        assert!(
            matches!(err, SignerError::Backend(m) if m.contains("local secret")),
            "expected Backend(local secret)"
        );
    }

    #[test]
    fn from_payload_round_trips_a_valid_payload() {
        // Baseline so the failure tests above prove something — a valid
        // payload restores and yields the same pubkey + relays.
        let remote_user = LocalKeySigner::generate();
        let (signer, transport) = build_signer_with_remote(&remote_user);
        let SignerPayload::Nip46(payload) = signer.to_payload() else {
            panic!("expected nip46 payload");
        };
        let restored = Nip46Signer::from_payload(&payload, transport).expect("valid restore");
        assert_eq!(restored.pubkey(), remote_user.pubkey());
        assert_eq!(restored.uri().relays, vec!["wss://relay.example.com".to_string()]);
    }

    // ---- handle accessors -------------------------------------------------

    #[test]
    fn handle_from_bunker_uri_propagates_parse_error() {
        // A malformed URI must surface the typed parse error, not panic.
        assert!(Nip46SignerHandle::from_bunker_uri("not-a-bunker-uri").is_err());
    }

    #[test]
    fn handle_with_explicit_local_key_uses_that_key() {
        // `from_bunker_uri_with_local_key` seeds a deterministic local key;
        // `local_pubkey()` must reflect it (used by tests that need a stable
        // ephemeral identity).
        let local = LocalKeySigner::generate();
        let local_sk = nostr::SecretKey::from_hex(local.secret_hex().as_str())
            .expect("valid secret hex");
        let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com");
        let handle =
            Nip46SignerHandle::from_bunker_uri_with_local_key(&uri, local_sk).expect("parse");
        assert_eq!(handle.local_pubkey(), local.pubkey());
        assert_eq!(handle.uri().remote_pubkey_hex, SAMPLE_PK);
    }
}
