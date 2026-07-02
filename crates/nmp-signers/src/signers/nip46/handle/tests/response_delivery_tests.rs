//! Core inbound-response lifecycle: `pubkey_hex`, `signer_kind`,
//! `deliver_response`, and `disconnect`.

use std::time::Duration;

use nmp_signer_iface::{RemoteSignerHandle, SignerError, UnsignedEvent};

use crate::signers::traits::Signer;
use crate::LocalKeySigner;

use super::fixtures::build_signer_with_remote;

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
fn deliver_response_resolves_pending_sign() {
    // Round-trip: start a sign() (Pending), feed back a real signed event
    // via deliver_response, observe the mapper-validated output.
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
    RemoteSignerHandle::deliver_response(&signer, &envelope);

    let signed = op
        .wait(Duration::from_secs(2))
        .expect("signed event arrives");
    assert_eq!(signed.id, real_signed.id);
    assert_eq!(signed.sig, real_signed.sig);
    assert_eq!(signed.unsigned.pubkey, remote_pubkey.to_hex());
}

#[test]
fn deliver_response_with_error_field_routes_rejected() {
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
    RemoteSignerHandle::deliver_response(&signer, &envelope);

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
fn deliver_response_with_invalid_json_is_dropped() {
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
    RemoteSignerHandle::deliver_response(&signer, "not json {{");
    // Missing id — silent drop.
    RemoteSignerHandle::deliver_response(&signer, r#"{"result":"x"}"#);

    // Now a real error envelope must still land.
    let envelope = serde_json::json!({
        "id": rpc_id,
        "error": "later",
    })
    .to_string();
    RemoteSignerHandle::deliver_response(&signer, &envelope);

    let err = op
        .wait(Duration::from_secs(2))
        .expect_err("error envelope must surface");
    assert!(matches!(err, SignerError::Rejected(_)));
}

#[test]
fn deliver_response_prefers_result_when_error_is_null() {
    // Some bunkers always include both fields; an explicit `error: null`
    // means "no error" — the `result` must win.  This pins the null-error
    // branch of `deliver_response`.
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
    RemoteSignerHandle::deliver_response(&signer, &envelope);

    let signed = op
        .wait(Duration::from_secs(2))
        .expect("null error must not block the result");
    assert_eq!(signed.id, real_signed.id);
}

#[test]
fn deliver_response_with_unknown_id_is_dropped() {
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
    RemoteSignerHandle::deliver_response(&signer, &envelope);

    // The real op must still be pending — the stray response did not
    // resolve it.
    assert!(
        op.poll().is_none(),
        "unknown-id response must not resolve a pending op"
    );
}
