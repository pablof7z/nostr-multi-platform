//! `Nip04`/`Nip44` RPC enqueue shape: method name, params, and round-trip
//! resolution via `resolve_response`.

use std::time::Duration;

use nmp_signer_iface::SignerError;

use crate::signers::traits::{Nip04, Nip44, Signer};
use crate::LocalKeySigner;

use super::fixtures::{build_signer_with_remote, single_rpc, SAMPLE_PK};

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
