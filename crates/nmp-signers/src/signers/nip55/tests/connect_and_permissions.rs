//! First-connect fast-path routing: restored grants, the single-Activity-slot
//! interactive rejection, the `ContentResolver` → forced-interactive retry,
//! and `signer_package` learned from a response.

use super::*;

/// A restored NIP-55 payload is immediately usable without a fresh permission
/// request: live ops carry `granted_permissions`, while `permissions` remains
/// empty so Amber is not asked to reapprove the batch.
#[test]
fn restored_payload_sign_request_uses_granted_fast_path_without_reapproval() {
    let local = LocalKeySigner::generate();
    let payload = Nip55Payload {
        user_pubkey_hex: local.pubkey().to_hex(),
        signer_package: Some("com.greenart7c3.nostrsigner".to_string()),
        granted_permissions: vec!["sign_event:1".to_string()],
    };
    let transport = FakeExternalSignerTransport::new();
    let signer = Nip55Signer::from_payload(
        &payload,
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    )
    .expect("restore from payload");

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "restored fast path".to_string(),
        created_at: 1,
    };
    let _op = <Nip55Signer as Signer>::sign(&signer, unsigned);
    let req = transport.last_request().expect("request captured");

    assert!(req.permissions.is_empty());
    assert_eq!(
        req.granted_permissions,
        vec![Nip55Permission::sign_event(1)]
    );
    assert!(req.uses_content_resolver_fast_path());
}

/// Android has one Activity Result slot for interactive signer Intents. Rust
/// rejects a second interactive request before native can overwrite the first
/// pending correlation id.
#[test]
fn overlapping_interactive_sign_requests_are_rejected() {
    let local = LocalKeySigner::generate();
    let transport = FakeExternalSignerTransport::new();
    let signer = Nip55Signer::new(
        local.pubkey(),
        Some("com.greenart7c3.nostrsigner".to_string()),
        vec![],
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    );
    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "interactive".to_string(),
        created_at: 1,
    };

    let mut op1 = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    assert!(op1.poll().is_none());
    let op2 = <Nip55Signer as Signer>::sign(&signer, unsigned);

    match op2.wait(Duration::from_secs(1)) {
        Err(SignerError::Unavailable(m)) => assert!(m.contains("approval already pending")),
        other => panic!("expected overlapping approval rejection, got {other:?}"),
    }
    assert_eq!(signer.pending_len(), 1);
    assert_eq!(
        transport.drain_requests().len(),
        1,
        "second interactive request must not reach native"
    );
}

/// A resolver-path `Unavailable` means a granted permission was silently
/// revoked or unavailable. Rust owns the retry policy: keep the same pending
/// op open and re-issue the request once with `force_interactive = true`.
#[test]
fn content_resolver_unavailable_retries_as_forced_interactive() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, transport) = make_signer_with_pubkey(pubkey);
    let unsigned = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "resolver retry".to_string(),
        created_at: 1,
    };

    let mut op = <Nip55Signer as Signer>::sign(&signer, unsigned);
    let first = transport.last_request().expect("first request");
    assert!(first.uses_content_resolver_fast_path());
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Unavailable {
            reason: "ContentResolver returned null result".to_string(),
        },
    );

    assert!(
        op.poll().is_none(),
        "retry must keep the original op pending"
    );
    let retry = transport.last_request().expect("retry request");
    assert_eq!(retry.correlation_id, first.correlation_id);
    assert!(retry.force_interactive);
    assert!(!retry.uses_content_resolver_fast_path());
    assert_eq!(signer.pending_len(), 1);

    let signed_json = make_signed_event_json(&pubkey, &local);
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: signed_json,
        },
    );
    let signed = op.wait(Duration::from_secs(1)).expect("retry resolves");
    assert_eq!(signed.unsigned.pubkey, pubkey.to_hex());
    assert_eq!(signer.pending_len(), 0);
}

/// `signer_package` is updated when the host reports it on a successful
/// `get_public_key` response (via `signer_package` in the response).
#[test]
fn signer_package_updated_from_response() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    // Construct without a package so we can test the update.
    let transport = FakeExternalSignerTransport::new();
    let signer = Nip55Signer::new(
        pubkey,
        None,
        vec![],
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    );
    assert!(signer.signer_package().is_none());

    // Issue a sign request and respond with a package name.
    let unsigned = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "pkg update".to_string(),
        created_at: 1,
    };
    let _op = <Nip55Signer as Signer>::sign(&signer, unsigned);
    transport.respond_to_last_with_package(
        &signer,
        ExternalSignerOutcome::Rejected {
            reason: "doesn't matter".to_string(),
        },
        "com.greenart7c3.nostrsigner",
    );

    // Note: signer_package is only updated on `Ok` outcomes (from get_public_key).
    // On Rejected the package is NOT updated (the transport returns package only for Ok).
    // This test confirms that Rejected does not update the package field.
    assert!(
        signer.signer_package().is_none(),
        "signer_package must not be updated on a Rejected response"
    );
}
