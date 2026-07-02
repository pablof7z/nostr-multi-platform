//! `Nip46Signer::from_payload` restore paths: cached-pubkey/local-secret
//! failure modes, plus the happy-path round trip.

use std::sync::Arc;

use nmp_signer_iface::SignerError;

use crate::signers::payload::SignerPayload;
use crate::signers::traits::Signer;
use crate::{LocalKeySigner, Nip46Signer};

use super::fixtures::{build_signer_with_remote, StubTransport};

#[test]
fn from_payload_without_cached_pubkey_returns_not_ready() {
    // A payload that has never completed a handshake has no cached remote
    // pubkey — restore must refuse with NotReady, not panic.
    let remote_user = LocalKeySigner::generate();
    let (signer, _t) = build_signer_with_remote(&remote_user);
    let SignerPayload::Nip46(mut payload) = signer.to_payload().expect("to_payload") else {
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
    let SignerPayload::Nip46(mut payload) = signer.to_payload().expect("to_payload") else {
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
    let SignerPayload::Nip46(mut payload) = signer.to_payload().expect("to_payload") else {
        panic!("expected nip46 payload");
    };
    payload.local_secret_hex = zeroize::Zeroizing::new("zzzz-not-a-secret".to_string());

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
    let SignerPayload::Nip46(payload) = signer.to_payload().expect("to_payload") else {
        panic!("expected nip46 payload");
    };
    let restored = Nip46Signer::from_payload(&payload, transport).expect("valid restore");
    assert_eq!(restored.pubkey(), remote_user.pubkey());
    assert_eq!(
        restored.uri().relays,
        vec!["wss://relay.example.com".to_string()]
    );
}
