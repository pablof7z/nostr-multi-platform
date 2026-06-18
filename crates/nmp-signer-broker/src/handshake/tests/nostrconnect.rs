//! Signer-initiated (`nostrconnect://`) handshake tests.

use super::*;
use crate::handshake::{run_nostrconnect_handshake, HandshakeError};

/// Security-critical: a `connect` frame whose `params[1]` secret does not
/// match the expected session secret must be rejected with a definitive
/// `BunkerError`, never accepted.
#[test]
fn run_nostrconnect_handshake_rejects_secret_mismatch() {
    let client_keys = Keys::generate();
    let signer_keys = Keys::generate();

    let (relay, _drop) = StubRelay::new();
    let (inbound_tx, inbound_rx) = crossbeam_channel::unbounded::<Value>();

    // Signer sends a connect frame with the WRONG secret.
    let bad = signer_connect_event(&signer_keys, client_keys.public_key(), "wrong-secret");
    inbound_tx.send(bad).unwrap();

    let cancel_rx = never_cancel();
    let err = run_nostrconnect_handshake(
        relay.as_ref(),
        &inbound_rx,
        &cancel_rx,
        &client_keys,
        "the-real-secret",
        &mut |_, _| {},
    )
    .expect_err("secret mismatch must abort");
    match err {
        HandshakeError::BunkerError(msg) => {
            assert!(
                msg.contains("secret mismatch"),
                "must report a secret mismatch, got: {msg:?}"
            );
        }
        other => panic!("expected BunkerError, got {other:?}"),
    }
}

/// Happy path for the signer-initiated (`nostrconnect://`) handshake:
/// valid connect with the right secret, then a `get_public_key` reply.
#[test]
fn run_nostrconnect_handshake_happy_path_returns_pubkeys() {
    let client_keys = Keys::generate();
    let signer_keys = Keys::generate();
    let user_keys = Keys::generate();
    let user_pk_hex = user_keys.public_key().to_hex();
    let secret = "session-secret-xyz";

    let (relay, frame_rx) = StubRelay::new();
    let (inbound_tx, inbound_rx) = crossbeam_channel::unbounded::<Value>();

    // Deliver the connect frame up front.
    let connect =
        signer_connect_event(&signer_keys, client_keys.public_key(), secret);
    inbound_tx.send(connect).unwrap();

    let cancel_rx = never_cancel();

    // Driver: block on each outgoing frame; after the broker publishes
    // `get_public_key`, reply with the user pubkey. The connect-ack is
    // also published; we only answer the get_public_key (the
    // decryptable RPC addressed to us). `recv()` blocks — no poll loop;
    // the loop exits when the relay is dropped at end-of-test.
    let signer_for_driver = signer_keys.clone();
    let client_pk = client_keys.public_key();
    let user_pk_for_driver = user_pk_hex.clone();
    let driver = std::thread::spawn(move || {
        while let Ok(frame) = frame_rx.recv() {
            let parsed: Value = serde_json::from_str(&frame).unwrap();
            let ct = parsed.as_array().unwrap()[1]
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap();
            // Try to decrypt; the broker encrypts to the signer.
            let Ok(plain) = nip44::decrypt(
                signer_for_driver.secret_key(),
                &client_pk,
                ct.as_bytes(),
            ) else {
                continue;
            };
            let req: Value = match serde_json::from_str(&plain) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // Only reply to the get_public_key request.
            if req.get("method").and_then(|v| v.as_str())
                == Some("get_public_key")
            {
                let req_id = req.get("id").and_then(|v| v.as_str()).unwrap();
                let good = make_response_event(
                    &signer_for_driver,
                    client_pk,
                    json!({"id": req_id, "result": user_pk_for_driver}),
                );
                let _ = inbound_tx.send(good);
            }
        }
    });

    let outcome = run_nostrconnect_handshake(
        relay.as_ref(),
        &inbound_rx,
        &cancel_rx,
        &client_keys,
        secret,
        &mut |_, _| {},
    )
    .expect("nostrconnect handshake completes");
    assert_eq!(outcome.signer_pubkey_hex, signer_keys.public_key().to_hex());
    assert_eq!(outcome.user_pubkey_hex, user_pk_hex);

    // Dropping the relay closes `frame_tx`; the driver's `recv()`
    // disconnects and the loop exits.
    drop(relay);
    let _ = driver.join();
}
