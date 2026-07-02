//! `nip44_encrypt`/`nip44_decrypt` round trips, and the
//! `to_payload`/`from_payload` persistence round-trip (including the
//! empty-grant-batch restore path, issue #2523).

use super::*;

/// `nip44_encrypt` returns `Pending` and resolves with the fake ciphertext.
#[test]
fn nip44_encrypt_round_trip() {
    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());
    let recipient = LocalKeySigner::generate().pubkey();

    let op = signer.nip44_encrypt(&recipient.to_hex(), "hello");
    assert!(
        matches!(op, SignerOp::Pending(_)),
        "nip44_encrypt must be Pending"
    );

    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: "fake-ciphertext".to_string(),
        },
    );

    let ct = op
        .wait(Duration::from_secs(1))
        .expect("encrypt must succeed");
    assert_eq!(ct, "fake-ciphertext");
}

/// `nip44_decrypt` returns `Pending` and resolves with the plaintext.
#[test]
fn nip44_decrypt_round_trip() {
    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());
    let sender = LocalKeySigner::generate().pubkey();

    let op = signer.nip44_decrypt(&sender.to_hex(), "fake-ciphertext");
    assert!(
        matches!(op, SignerOp::Pending(_)),
        "nip44_decrypt must be Pending"
    );

    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: "hello".to_string(),
        },
    );

    let pt = op
        .wait(Duration::from_secs(1))
        .expect("decrypt must succeed");
    assert_eq!(pt, "hello");
}

/// Invalid recipient pubkey surfaces as `SignerError::Backend` (D6 — no panic).
#[test]
fn nip44_encrypt_invalid_pubkey_returns_backend_error() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());

    let op = signer.nip44_encrypt("not-a-pubkey", "hello");
    match op.wait(Duration::from_secs(1)) {
        Err(SignerError::Backend(m)) => assert!(m.contains("recipient")),
        other => panic!("expected Backend(recipient), got {other:?}"),
    }
}

/// `nip44_decrypt` with an invalid sender pubkey → `Backend` error.
#[test]
fn nip44_decrypt_invalid_pubkey_returns_backend_error() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());

    let op = signer.nip44_decrypt("not-a-pubkey", "ct");
    match op.wait(Duration::from_secs(1)) {
        Err(SignerError::Backend(m)) => assert!(m.contains("sender")),
        other => panic!("expected Backend(sender), got {other:?}"),
    }
}

/// Persistence round-trip: `to_payload` → `SignerPayload::Nip55` → `from_payload`.
#[test]
fn persistence_round_trip() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, _transport) = make_signer_with_pubkey(pubkey);

    let payload = signer.to_payload().expect("to_payload");
    let SignerPayload::Nip55(p) = &payload else {
        panic!("expected SignerPayload::Nip55, got {payload:?}");
    };
    assert_eq!(p.user_pubkey_hex, pubkey.to_hex());
    assert_eq!(
        p.signer_package,
        Some("com.greenart7c3.nostrsigner".to_string())
    );
    assert!(!p.granted_permissions.is_empty());

    // Restore from the payload.
    let transport2 = FakeExternalSignerTransport::new();
    let restored = Nip55Signer::from_payload(
        p,
        Arc::clone(&transport2) as Arc<dyn ExternalSignerTransport>,
    )
    .expect("from_payload must succeed");

    assert_eq!(restored.pubkey_hex(), pubkey.to_hex());
    assert_eq!(
        restored.signer_package(),
        Some("com.greenart7c3.nostrsigner".to_string())
    );
}

/// An empty persisted permission batch restores with an empty granted set —
/// `from_payload` must NOT synthesize a framework default (issue #2523 /
/// crate-boundaries.md §9). The signer self-heals via interactive re-grant on
/// first use.
#[test]
fn from_payload_with_empty_persisted_batch_restores_with_empty_grants() {
    let local = LocalKeySigner::generate();
    let payload = Nip55Payload {
        user_pubkey_hex: local.pubkey().to_hex(),
        signer_package: Some("com.greenart7c3.nostrsigner".to_string()),
        granted_permissions: Vec::new(),
    };
    let transport = FakeExternalSignerTransport::new();
    let signer = Nip55Signer::from_payload(
        &payload,
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    )
    .expect("restore from payload");

    let restored_payload = signer.to_payload().expect("to_payload");
    let SignerPayload::Nip55(p) = restored_payload else {
        panic!("expected Nip55 payload");
    };
    assert!(
        p.granted_permissions.is_empty(),
        "restoring an empty persisted batch must not synthesize a default"
    );

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "no grants yet".to_string(),
        created_at: 1,
    };
    let _op = <Nip55Signer as Signer>::sign(&signer, unsigned);
    let req = transport.last_request().expect("request captured");
    assert!(
        req.granted_permissions.is_empty(),
        "live request must carry no grants when restored with an empty batch"
    );
}

/// `from_payload` fails gracefully on an invalid pubkey hex (D6).
#[test]
fn from_payload_invalid_pubkey_returns_err() {
    let transport = FakeExternalSignerTransport::new();
    let bad = Nip55Payload {
        user_pubkey_hex: "not-a-valid-hex".to_string(),
        signer_package: None,
        granted_permissions: vec![],
    };
    let result = Nip55Signer::from_payload(
        &bad,
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    );
    assert!(
        matches!(result, Err(SignerError::Backend(_))),
        "invalid pubkey hex must yield Backend error"
    );
}

/// `persistence_payload_json()` (from `RemoteSignerHandle`) serialises to
/// valid JSON that round-trips through `serde_json`.
#[test]
fn persistence_payload_json_round_trips() {
    let local = LocalKeySigner::generate();
    let (signer, _) = make_signer_with_pubkey(local.pubkey());

    let json = signer
        .persistence_payload_json()
        .expect("must produce Some");
    let parsed: SignerPayload = serde_json::from_str(&json).expect("must parse");
    let SignerPayload::Nip55(p) = parsed else {
        panic!("expected Nip55 payload");
    };
    assert_eq!(p.user_pubkey_hex, local.pubkey().to_hex());
}

/// ADR-0072 §D5 + V-08 degrade pin: `nip44_decrypt` is implemented in
/// `Nip55Signer` (the capability is complete) but is NOT wired to the
/// DM-inbox path (deferred to V-08/#961).
///
/// This test pins the staging boundary: the method is callable in isolation,
/// but the receive-side infrastructure does not call it.  When V-08 is
/// implemented, this test can be upgraded to an end-to-end receive test.
#[test]
fn nip55_nip44_decrypt_capability_exists_but_receive_path_is_deferred() {
    use nmp_signer_iface::RemoteSignerHandle;
    use nmp_signer_iface::{ExternalSignerOutcome, SignerOp};
    use std::time::Duration;

    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());

    let sender = LocalKeySigner::generate().pubkey();

    // nip44_decrypt is callable and returns Pending (not an error stub).
    let op = signer.nip44_decrypt(&sender.to_hex(), "fake-ciphertext");
    assert!(
        matches!(op, SignerOp::Pending(_)),
        "nip44_decrypt must return Pending (not an error stub) — the capability is implemented"
    );

    // Answer the request (proves the pending chain resolves correctly).
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: "decrypted-plaintext".to_string(),
        },
    );

    let pt = op
        .wait(Duration::from_secs(1))
        .expect("nip44_decrypt must resolve when answered");
    assert_eq!(
        pt, "decrypted-plaintext",
        "resolved plaintext must match what the fake transport returned"
    );

    // Staging note: V-08/#961 is where the DM-inbox path is wired to use this.
    // The method is intentionally not called from the kernel's DM-inbox today
    // (staged with V-08 per ADR-0072 D5). The test above confirms the
    // `RemoteSignerHandle::nip44_decrypt` seam works for NIP-55 in isolation.
}
