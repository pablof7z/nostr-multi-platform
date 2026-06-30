//! `deliver_external_response` outcomes (rejected / unavailable / unknown
//! correlation id / malformed JSON), `disconnect`, transport-send failure
//! cleanup, and independent multi-op resolution.

use super::*;

/// A rejected response resolves the pending op with `SignerError::Rejected`.
#[test]
fn rejected_response_resolves_with_rejected_error() {
    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "will reject".to_string(),
        created_at: 1,
    };
    let op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Rejected {
            reason: "user cancelled".to_string(),
        },
    );
    match op.wait(Duration::from_secs(1)) {
        Err(SignerError::Rejected(m)) => assert_eq!(m, "user cancelled"),
        other => panic!("expected Rejected, got {other:?}"),
    }
}

/// An `Unavailable` response resolves the op with `SignerError::Unavailable`.
#[test]
fn unavailable_response_resolves_with_unavailable_error() {
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
        content: "app uninstalled".to_string(),
        created_at: 1,
    };
    let op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Unavailable {
            reason: "signer not installed".to_string(),
        },
    );
    match op.wait(Duration::from_secs(1)) {
        Err(SignerError::Unavailable(m)) => assert_eq!(m, "signer not installed"),
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

/// An unknown correlation id in a `deliver_response` call is silently dropped
/// (D6 — degrades to timeout for the real pending op, not an error).
#[test]
fn unknown_correlation_id_is_dropped_silently() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "pending".to_string(),
        created_at: 1,
    };
    let mut op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());

    // Deliver a response with a non-matching correlation_id.
    let resp = ExternalSignerResponse {
        correlation_id: "unknown-id".to_string(),
        outcome: ExternalSignerOutcome::Ok {
            result: "{}".to_string(),
        },
        signer_package: None,
    };
    signer.deliver_external_response(&serde_json::to_string(&resp).unwrap());

    // The pending op must still be pending (not resolved).
    assert!(
        op.poll().is_none(),
        "pending op must not resolve on unknown correlation_id"
    );
    assert_eq!(signer.pending_len(), 1);
}

/// Malformed (non-JSON) `deliver_response` is dropped without panicking (D6).
#[test]
fn malformed_deliver_response_is_dropped() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "pending".to_string(),
        created_at: 1,
    };
    let mut op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());

    // Deliver malformed input — must not panic.
    signer.deliver_external_response("not json at all {{{");

    // The pending op must still be pending.
    assert!(
        op.poll().is_none(),
        "pending op must not resolve on malformed deliver_response"
    );
    assert_eq!(signer.pending_len(), 1);
}

/// `disconnect()` drains all pending ops with `SignerError::Rejected`.
#[test]
fn disconnect_drains_pending_ops_with_error() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "pending".to_string(),
        created_at: 1,
    };
    let op1 = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    let op2 = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    assert_eq!(signer.pending_len(), 2);

    signer.disconnect();

    assert_eq!(signer.pending_len(), 0, "pending must be drained");
    for op in [op1, op2] {
        match op.wait(Duration::from_secs(1)) {
            Err(SignerError::Rejected(m)) => assert!(
                m.contains("disconnected"),
                "disconnect error message must contain 'disconnected'"
            ),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }
}

/// Transport send failure: pending entry is cleaned up immediately.
#[test]
fn transport_send_failure_cleans_up_pending() {
    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());

    transport.fail_next(SignerError::Backend("no route".to_string()));

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "will fail send".to_string(),
        created_at: 1,
    };
    let op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    assert_eq!(
        signer.pending_len(),
        0,
        "failed send must not leak pending entry"
    );
    match op.wait(Duration::from_secs(1)) {
        Err(SignerError::Backend(m)) => assert_eq!(m, "no route"),
        other => panic!("expected Backend, got {other:?}"),
    }
}

/// Multiple pending ops are independent: resolving one does not affect others.
#[test]
fn multiple_pending_ops_resolved_independently() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, transport) = make_signer_with_pubkey(pubkey);

    let unsigned1 = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "op1".to_string(),
        created_at: 1,
    };
    let unsigned2 = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "op2".to_string(),
        created_at: 2,
    };

    let op1 = <Nip55Signer as Signer>::sign(&signer, unsigned1.clone());
    let req1 = transport.last_request().expect("req1");
    let _op2 = <Nip55Signer as Signer>::sign(&signer, unsigned2.clone());
    assert_eq!(signer.pending_len(), 2);

    // Resolve only op1 by echoing its correlation_id.
    let signed_json = make_signed_event_json(&pubkey, &local);
    let resp1 = ExternalSignerResponse {
        correlation_id: req1.correlation_id.clone(),
        outcome: ExternalSignerOutcome::Ok {
            result: signed_json,
        },
        signer_package: None,
    };
    signer.deliver_external_response(&serde_json::to_string(&resp1).unwrap());

    let signed1 = op1.wait(Duration::from_secs(1)).expect("op1 must resolve");
    assert_eq!(signed1.unsigned.content, "nip55 test"); // local sign's content
    assert_eq!(
        signer.pending_len(),
        1,
        "op2 must remain pending after op1 resolves"
    );
}
