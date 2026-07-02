//! `enqueue`'s transmit-failure branch: a `Nip46Transport::send_rpc` error
//! must surface as a `SignerOp::err` (D6 — never a panic or a hang), and
//! must never leak a pending entry.

use std::sync::Arc;
use std::time::Duration;

use nmp_signer_iface::{RemoteSignerHandle, SignerError, UnsignedEvent};

use crate::signers::traits::{Nip04, Signer};
use crate::{LocalKeySigner, Nip46Signer, Nip46SignerHandle};

use super::fixtures::{FailingTransport, SAMPLE_PK};

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
        let _ = <Nip46Signer as Signer>::sign(&signer, unsigned.clone());
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
