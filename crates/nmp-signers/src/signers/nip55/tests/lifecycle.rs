//! Construction, identity, `op_timeout`, the sign round-trip, the outbound
//! request shape, and the `Debug` impl.

use super::*;

/// `op_timeout()` must return 90s — the NIP-55 Intent round-trip budget.
#[test]
fn op_timeout_is_90s() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());
    assert_eq!(
        RemoteSignerHandle::op_timeout(&signer),
        EXTERNAL_SIGN_TIMEOUT,
        "Nip55Signer must report 90s op_timeout (ADR-0072 D4)"
    );
}

/// `signer_kind()` must return `"nip55"`.
#[test]
fn signer_kind_is_nip55() {
    let local = LocalKeySigner::generate();
    let (signer, _) = make_signer_with_pubkey(local.pubkey());
    assert_eq!(signer.signer_kind(), "nip55");
}

/// `pubkey_hex()` returns the user pubkey we constructed with.
#[test]
fn pubkey_hex_matches_construction_pubkey() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, _) = make_signer_with_pubkey(pubkey);
    assert_eq!(signer.pubkey_hex(), pubkey.to_hex());
}

/// A sign op returns `SignerOp::Pending` immediately (not `Ready`); it is
/// non-blocking (D8). Polling before the response arrives yields `None`.
#[test]
fn sign_returns_pending_before_response() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());
    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "test".to_string(),
        created_at: 1_700_000_000,
    };
    let mut op = <Nip55Signer as RemoteSignerHandle>::sign(&signer, &unsigned);
    assert!(
        op.poll().is_none(),
        "sign must return Pending before response arrives"
    );
    assert_eq!(signer.pending_len(), 1, "one pending entry registered");
}

/// Full sign round-trip: the fake transport captures the request, we inject a
/// valid signed-event JSON, and the op resolves with the correct event.
#[test]
fn sign_round_trip_via_fake_transport() {
    // Use the same local key as the "external signer" so we can produce a
    // real verifiable signature for the mapper to check.
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, transport) = make_signer_with_pubkey(pubkey);

    let unsigned = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "nip55 round-trip".to_string(),
        created_at: 1_700_000_000,
    };

    let op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    assert!(matches!(op, SignerOp::Pending(_)), "must be Pending");

    // Inject a valid signed-event JSON response.
    let signed_json = make_signed_event_json(&pubkey, &local);
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: signed_json,
        },
    );

    let signed = op.wait(Duration::from_secs(5)).expect("sign must succeed");
    // The mapper trusts the response's own fields (codex review #3 trust model).
    // `make_signed_event_json` signed content="nip55 test" — that's what the
    // response JSON carries, so that's what the mapper returns.
    assert_eq!(signed.unsigned.content, "nip55 test");
    assert_eq!(signed.unsigned.pubkey, pubkey.to_hex());
    assert_eq!(signer.pending_len(), 0, "pending entry must be cleaned up");
}

/// The outbound request carries `method: sign_event`, the correct `payload`
/// (unsigned event JSON), and `current_user` set to the user's pubkey.
#[test]
fn sign_request_shape() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, transport) = make_signer_with_pubkey(pubkey);

    let unsigned = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "shape check".to_string(),
        created_at: 1_700_000_001,
    };
    let _ = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    let req = transport.last_request().expect("request captured");

    assert_eq!(
        req.method,
        nmp_signer_iface::ExternalSignerMethod::SignEvent
    );
    assert_eq!(req.current_user, Some(pubkey.to_hex()));
    assert!(
        req.payload.contains("shape check"),
        "payload must contain the unsigned content"
    );
    assert!(!req.correlation_id.is_empty(), "correlation_id must be set");
    assert!(
        req.permissions.is_empty(),
        "live sign requests must not re-request the permission batch"
    );
    assert!(
        req.granted_permissions
            .iter()
            .any(|p| p.kind == "sign_event:1"),
        "live sign requests must carry persisted grant facts for the resolver fast-path"
    );
    assert!(
        req.uses_content_resolver_fast_path(),
        "restored/live signer requests with grants should avoid a Connect dialog"
    );
}

/// `Debug` impl does not panic and shows non-secret fields.
#[test]
fn debug_impl_does_not_panic() {
    let local = LocalKeySigner::generate();
    let (signer, _) = make_signer_with_pubkey(local.pubkey());
    let debug_str = format!("{signer:?}");
    assert!(debug_str.contains("Nip55Signer"));
    assert!(debug_str.contains(&local.pubkey().to_hex()));
}
