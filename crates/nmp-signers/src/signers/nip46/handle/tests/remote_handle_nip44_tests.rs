//! `RemoteSignerHandle::nip44_*` seam (ADR-0072): the actor-facing methods
//! parse hex pubkeys and delegate to the inner `Nip44` impl.

use std::time::Duration;

use nmp_signer_iface::{RemoteSignerHandle, SignerError};

use crate::signers::traits::Signer;
use crate::LocalKeySigner;

use super::fixtures::{build_signer_with_remote, single_rpc};

#[test]
fn remote_handle_nip44_encrypt_queues_rpc_and_round_trips() {
    // ADR-0072: the actor-facing `RemoteSignerHandle::nip44_encrypt` parses
    // hex, then delegates to the inner `Nip44` impl — the RPC must carry
    // the `nip44_encrypt` method and the opaque ciphertext result must
    // surface verbatim (no verify() step).
    let remote_user = LocalKeySigner::generate();
    let (signer, transport) = build_signer_with_remote(&remote_user);
    let recipient = LocalKeySigner::generate().pubkey();

    let op = RemoteSignerHandle::nip44_encrypt(&signer, &recipient.to_hex(), "seal plaintext");
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

    let _op = RemoteSignerHandle::nip44_decrypt(&signer, &sender.to_hex(), "sealed-payload");
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
