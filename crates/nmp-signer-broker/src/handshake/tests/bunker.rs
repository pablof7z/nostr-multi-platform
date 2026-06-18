//! Client-initiated (`bunker://`) handshake tests, plus the `await_response`
//! error / robustness paths.

use super::*;
use crate::handshake::{run_handshake, HandshakeError};

#[test]
fn happy_path_connect_then_get_public_key_returns_user_pubkey() {
    let client_keys = Keys::generate();
    let bunker_keys = Keys::generate();
    let bunker_pubkey = bunker_keys.public_key();
    let user_keys = Keys::generate();
    let user_pk_hex = user_keys.public_key().to_hex();

    let (relay, frame_rx) = StubRelay::new();
    let (inbound_tx, inbound_rx) = crossbeam_channel::unbounded::<Value>();
    let cancel_rx = never_cancel();

    // Driver thread: block on each outgoing frame as it is published,
    // manufacture the matching bunker response, push it onto the inbound
    // channel. `recv()` blocks (no poll loop); the loop ends naturally
    // when the relay is dropped at end-of-test and `recv()` disconnects.
    let bunker_keys_for_driver = bunker_keys.clone();
    let client_pk_for_driver = client_keys.public_key();
    let user_pk_for_driver = user_pk_hex.clone();
    let driver = std::thread::spawn(move || {
        let mut seen = 0usize;
        while let Ok(frame) = frame_rx.recv() {
            // Frame 0 is `connect` (reply "ack"); frame 1 is
            // `get_public_key` (reply user pubkey).
            let result = if seen == 0 {
                "ack".to_string()
            } else {
                user_pk_for_driver.clone()
            };
            let response = bunker_response(
                &frame,
                &bunker_keys_for_driver,
                client_pk_for_driver,
                &result,
            );
            let _ = inbound_tx.send(response);
            seen += 1;
        }
    });

    let mut progress_events: Vec<(String, Option<String>)> = Vec::new();
    let outcome = run_handshake(
        relay.as_ref(),
        &inbound_rx,
        &cancel_rx,
        &client_keys,
        bunker_pubkey,
        None,
        None,
        &mut |stage, msg| progress_events.push((stage.to_string(), msg.map(String::from))),
    )
    .expect("handshake completes");

    assert_eq!(outcome.user_pubkey_hex, user_keys.public_key().to_hex());
    assert!(progress_events.iter().any(|(s, _)| s == "connecting"));
    assert!(progress_events.iter().any(|(s, _)| s == "awaiting_pubkey"));
    assert!(relay.last_event().is_some());

    // Wind the driver down: dropping the relay closes `frame_tx`, so the
    // driver's `recv()` disconnects and the loop exits deterministically.
    drop(relay);
    let _ = driver.join();
}

#[test]
fn cancellation_aborts_with_cancelled_error() {
    let client_keys = Keys::generate();
    let bunker_pk = Keys::generate().public_key();

    let (relay, frame_rx) = StubRelay::new();
    let (_inbound_tx, inbound_rx) = crossbeam_channel::unbounded::<Value>();
    let (cancel_tx, cancel_rx) = crossbeam_channel::bounded::<()>(1);

    // Deterministic trigger: block until the handshake publishes its first
    // outgoing frame (the `connect` RPC), then cancel by sending on the cancel
    // channel. The handshake `select!` wakes immediately on the cancel arm —
    // event-driven, with no inbound traffic and no timer. No sleep needed.
    let canceller = std::thread::spawn(move || {
        let _ = frame_rx.recv();
        let _ = cancel_tx.send(());
    });

    let err = run_handshake(
        relay.as_ref(),
        &inbound_rx,
        &cancel_rx,
        &client_keys,
        bunker_pk,
        None,
        None,
        &mut |_, _| {},
    )
    .expect_err("cancelled");
    assert!(matches!(err, HandshakeError::Cancelled));
    let _ = canceller.join();
}

/// The security-critical path: when the bunker replies with an `error`
/// field, the handshake must surface a `BunkerError` carrying the text —
/// never silently treat it as success.
#[test]
fn run_handshake_surfaces_bunker_error_response() {
    let client_keys = Keys::generate();
    let bunker_keys = Keys::generate();
    let bunker_pubkey = bunker_keys.public_key();

    let (relay, frame_rx) = StubRelay::new();
    let (inbound_tx, inbound_rx) = crossbeam_channel::unbounded::<Value>();

    let cancel_rx = never_cancel();

    // Driver: block until the first outgoing frame (the `connect` RPC)
    // arrives, then reply with an explicit error payload. `recv()`
    // blocks — no poll loop.
    let bunker_for_driver = bunker_keys.clone();
    let client_pk = client_keys.public_key();
    let driver = std::thread::spawn(move || {
        if let Ok(frame) = frame_rx.recv() {
            // Extract the connect request id by decrypting the frame.
            let parsed: Value = serde_json::from_str(&frame).unwrap();
            let ct = parsed.as_array().unwrap()[1]
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap();
            let plain = nip44::decrypt(
                bunker_for_driver.secret_key(),
                &client_pk,
                ct.as_bytes(),
            )
            .unwrap();
            let req: Value = serde_json::from_str(&plain).unwrap();
            let req_id = req.get("id").and_then(|v| v.as_str()).unwrap();
            let err_rpc = json!({
                "id": req_id,
                "result": Value::Null,
                "error": "user rejected the request",
            });
            let event =
                make_response_event(&bunker_for_driver, client_pk, err_rpc);
            let _ = inbound_tx.send(event);
        }
    });

    let err = run_handshake(
        relay.as_ref(),
        &inbound_rx,
        &cancel_rx,
        &client_keys,
        bunker_pubkey,
        None,
        None,
        &mut |_, _| {},
    )
    .expect_err("bunker error must abort the handshake");
    match err {
        HandshakeError::BunkerError(msg) => {
            assert!(
                msg.contains("user rejected"),
                "error text must reach the caller, got: {msg:?}"
            );
        }
        other => panic!("expected BunkerError, got {other:?}"),
    }

    let _ = driver.join();
}

/// A response carrying a non-string `result` (e.g. a bare object) must be
/// surfaced as a `Protocol` error, not silently accepted.
#[test]
fn run_handshake_rejects_non_string_result() {
    let client_keys = Keys::generate();
    let bunker_keys = Keys::generate();
    let bunker_pubkey = bunker_keys.public_key();

    let (relay, frame_rx) = StubRelay::new();
    let (inbound_tx, inbound_rx) = crossbeam_channel::unbounded::<Value>();

    let cancel_rx = never_cancel();

    // Driver: block for the first outgoing frame, then reply with a
    // malformed (non-string `result`) payload. `recv()` blocks.
    let bunker_for_driver = bunker_keys.clone();
    let client_pk = client_keys.public_key();
    let driver = std::thread::spawn(move || {
        if let Ok(frame) = frame_rx.recv() {
            let parsed: Value = serde_json::from_str(&frame).unwrap();
            let ct = parsed.as_array().unwrap()[1]
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap();
            let plain = nip44::decrypt(
                bunker_for_driver.secret_key(),
                &client_pk,
                ct.as_bytes(),
            )
            .unwrap();
            let req: Value = serde_json::from_str(&plain).unwrap();
            let req_id = req.get("id").and_then(|v| v.as_str()).unwrap();
            // `result` is an object, not a string.
            let bad_rpc = json!({ "id": req_id, "result": {"unexpected": true} });
            let event =
                make_response_event(&bunker_for_driver, client_pk, bad_rpc);
            let _ = inbound_tx.send(event);
        }
    });

    let err = run_handshake(
        relay.as_ref(),
        &inbound_rx,
        &cancel_rx,
        &client_keys,
        bunker_pubkey,
        None,
        None,
        &mut |_, _| {},
    )
    .expect_err("non-string result must abort the handshake");
    assert!(
        matches!(err, HandshakeError::Protocol(_)),
        "expected Protocol error, got {err:?}"
    );

    let _ = driver.join();
}

/// Stray events (wrong pubkey, undecryptable content) must be skipped
/// without panic or premature failure; the genuine response that arrives
/// afterward must still complete the step. Exercises D6 robustness.
#[test]
fn run_handshake_skips_stray_events_then_completes() {
    let client_keys = Keys::generate();
    let bunker_keys = Keys::generate();
    let bunker_pubkey = bunker_keys.public_key();
    let user_keys = Keys::generate();
    let user_pk_hex = user_keys.public_key().to_hex();
    let stranger = Keys::generate();

    let (relay, frame_rx) = StubRelay::new();
    let (inbound_tx, inbound_rx) = crossbeam_channel::unbounded::<Value>();

    let cancel_rx = never_cancel();

    // Driver: block on each outgoing frame; for every one, inject noise
    // (stranger event + garbage ciphertext) ahead of the genuine reply.
    // `recv()` blocks; the loop exits when the relay is dropped.
    let bunker_for_driver = bunker_keys.clone();
    let client_pk = client_keys.public_key();
    let user_pk_for_driver = user_pk_hex.clone();
    let driver = std::thread::spawn(move || {
        let mut seen = 0usize;
        while let Ok(frame) = frame_rx.recv() {
            // Inject noise BEFORE the genuine reply: an event from a
            // stranger and an event with garbage content.
            let stray = make_response_event(
                &stranger,
                client_pk,
                json!({"id": "noise", "result": "ignored"}),
            );
            let _ = inbound_tx.send(stray);
            let mut garbage = make_response_event(
                &bunker_for_driver,
                client_pk,
                json!({"id": "noise2", "result": "x"}),
            );
            garbage["content"] = json!("not-real-ciphertext");
            let _ = inbound_tx.send(garbage);

            // Now the genuine reply.
            let parsed: Value = serde_json::from_str(&frame).unwrap();
            let ct = parsed.as_array().unwrap()[1]
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap();
            let plain = nip44::decrypt(
                bunker_for_driver.secret_key(),
                &client_pk,
                ct.as_bytes(),
            )
            .unwrap();
            let req: Value = serde_json::from_str(&plain).unwrap();
            let req_id =
                req.get("id").and_then(|v| v.as_str()).unwrap().to_string();
            let result = if seen == 0 {
                "ack".to_string()
            } else {
                user_pk_for_driver.clone()
            };
            let good = make_response_event(
                &bunker_for_driver,
                client_pk,
                json!({"id": req_id, "result": result}),
            );
            let _ = inbound_tx.send(good);
            seen += 1;
        }
    });

    let outcome = run_handshake(
        relay.as_ref(),
        &inbound_rx,
        &cancel_rx,
        &client_keys,
        bunker_pubkey,
        None,
        None,
        &mut |_, _| {},
    )
    .expect("handshake completes despite stray events");
    assert_eq!(outcome.user_pubkey_hex, user_pk_hex);

    // Dropping the relay closes `frame_tx`; the driver's `recv()`
    // disconnects and the loop exits.
    drop(relay);
    let _ = driver.join();
}
