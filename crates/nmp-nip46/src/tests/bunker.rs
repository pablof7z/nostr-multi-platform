//! Synchronous reducer tests for the client-initiated (`bunker://`) handshake.
//!
//! No threads, no channels, no blocking — each test feeds events directly to
//! the reducer and asserts the returned effects.

use nostr::Keys;
use serde_json::{json, Value};

use super::*;
use crate::bunker::start_bunker;
use crate::effect::Effect;
use crate::error::HandshakeError;

const SUB_ID: &str = "nmp-bunker";

/// Helper: run `start_bunker` and collect Send/Subscribe frames.
fn bunker_start(
    client_keys: &Keys,
    bunker_pubkey: nostr::PublicKey,
    secret: Option<&str>,
    perms: Option<&str>,
) -> (crate::reducer::SessionState, Vec<Effect>) {
    start_bunker(
        SUB_ID,
        client_keys.clone(),
        bunker_pubkey,
        "wss://relay.example.com".to_string(),
        secret,
        perms,
        TEST_NOW,
    )
}

// ─── Happy path ──────────────────────────────────────────────────────────────

/// End-to-end happy path: the reducer produces a connect SendFrame, and when
/// the bunker replies with "ack", it advances to WaitGpk and emits a
/// get_public_key SendFrame.  On the gpk reply it emits SignerReady.
#[test]
fn happy_path_connect_then_get_public_key_returns_signer_ready() {
    let client_keys = Keys::generate();
    let bunker_keys = Keys::generate();
    let user_keys = Keys::generate();
    let user_pk_hex = user_keys.public_key().to_hex();

    // ── start ──
    let (mut state, effects) = bunker_start(&client_keys, bunker_keys.public_key(), None, None);

    // Expect: Subscribe, Progress("connecting"), SendFrame(connect)
    assert!(matches!(&effects[0], Effect::Subscribe { .. }));
    assert!(matches!(&effects[1], Effect::Progress { stage, .. } if stage == "connecting"));
    let connect_frame = match &effects[2] {
        Effect::SendFrame { text, .. } => text.clone(),
        other => panic!("expected SendFrame, got {other:?}"),
    };

    // ── connect response ──
    let ack_event = respond_to_frame(
        &connect_frame,
        &bunker_keys,
        client_keys.public_key(),
        "ack",
    );
    let effects2 = state.on_relay_event(&ack_event, TEST_NOW);

    // Expect: Progress("awaiting_pubkey"), SendFrame(get_public_key)
    assert!(
        matches!(&effects2[0], Effect::Progress { stage, .. } if stage == "awaiting_pubkey"),
        "expected awaiting_pubkey progress, got {effects2:?}"
    );
    let gpk_frame = match &effects2[1] {
        Effect::SendFrame { text, .. } => text.clone(),
        other => panic!("expected SendFrame for get_public_key, got {other:?}"),
    };

    // ── get_public_key response ──
    let gpk_event = respond_to_frame(
        &gpk_frame,
        &bunker_keys,
        client_keys.public_key(),
        &user_pk_hex,
    );
    let effects3 = state.on_relay_event(&gpk_event, TEST_NOW);

    // Expect: SignerReady
    let sr = match effects3.into_iter().next() {
        Some(Effect::SignerReady(sr)) => sr,
        other => panic!("expected SignerReady, got {other:?}"),
    };
    assert_eq!(sr.user_pubkey_hex, user_pk_hex);
    assert_eq!(
        sr.remote_signer_pubkey_hex,
        bunker_keys.public_key().to_hex()
    );
    assert!(sr.granted_perms.is_none());
}

// ─── Progress events ─────────────────────────────────────────────────────────

#[test]
fn start_bunker_emits_connecting_progress_with_code() {
    let client_keys = Keys::generate();
    let bunker_pk = Keys::generate().public_key();
    let (_, effects) = bunker_start(&client_keys, bunker_pk, None, None);

    let progress = effects.iter().find_map(|e| {
        if let Effect::Progress { stage, code, .. } = e {
            if stage == "connecting" {
                Some(code.clone())
            } else {
                None
            }
        } else {
            None
        }
    });
    assert!(progress.is_some(), "must emit connecting progress");
    assert!(
        progress.unwrap().as_deref() == Some(crate::progress_codes::SENDING_CONNECT_TO_BUNKER),
        "must carry the stable progress code"
    );
}

// ─── Error paths ─────────────────────────────────────────────────────────────

/// The security-critical path: when the bunker replies with an `error` field,
/// the reducer must surface `Effect::Error(BunkerError(...))`.
#[test]
fn bunker_error_response_to_connect_produces_error_effect() {
    let client_keys = Keys::generate();
    let bunker_keys = Keys::generate();

    let (mut state, effects) = bunker_start(&client_keys, bunker_keys.public_key(), None, None);
    let connect_frame = match &effects[2] {
        Effect::SendFrame { text, .. } => text.clone(),
        other => panic!("expected SendFrame, got {other:?}"),
    };

    // Extract connect request id, send bunker error.
    let parsed: Value = serde_json::from_str(&connect_frame).unwrap();
    let ct = parsed.as_array().unwrap()[1]
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap();
    let plain = nostr::nips::nip44::decrypt(
        bunker_keys.secret_key(),
        &client_keys.public_key(),
        ct.as_bytes(),
    )
    .unwrap();
    let req: Value = serde_json::from_str(&plain).unwrap();
    let req_id = req.get("id").and_then(|v| v.as_str()).unwrap();
    let err_rpc =
        json!({ "id": req_id, "result": Value::Null, "error": "user rejected the request" });
    let event = make_response_event(&bunker_keys, client_keys.public_key(), err_rpc);

    let result_effects = state.on_relay_event(&event, TEST_NOW);
    let err = match result_effects.into_iter().next() {
        Some(Effect::Error { error }) => error,
        other => panic!("expected Error effect, got {other:?}"),
    };
    assert!(
        matches!(&err, HandshakeError::BunkerError(msg) if msg.contains("user rejected")),
        "expected BunkerError with rejection message, got {err:?}"
    );
}

/// A response carrying a non-string `result` must be surfaced as a
/// `Protocol` error, not silently accepted.
#[test]
fn non_string_result_produces_protocol_error() {
    let client_keys = Keys::generate();
    let bunker_keys = Keys::generate();

    let (mut state, effects) = bunker_start(&client_keys, bunker_keys.public_key(), None, None);
    let connect_frame = match &effects[2] {
        Effect::SendFrame { text, .. } => text.clone(),
        other => panic!("expected SendFrame, got {other:?}"),
    };

    let parsed: Value = serde_json::from_str(&connect_frame).unwrap();
    let ct = parsed.as_array().unwrap()[1]
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap();
    let plain = nostr::nips::nip44::decrypt(
        bunker_keys.secret_key(),
        &client_keys.public_key(),
        ct.as_bytes(),
    )
    .unwrap();
    let req: Value = serde_json::from_str(&plain).unwrap();
    let req_id = req.get("id").and_then(|v| v.as_str()).unwrap();
    let bad_rpc = json!({ "id": req_id, "result": {"unexpected": true} });
    let event = make_response_event(&bunker_keys, client_keys.public_key(), bad_rpc);

    let result_effects = state.on_relay_event(&event, TEST_NOW);
    let err = match result_effects.into_iter().next() {
        Some(Effect::Error { error }) => error,
        other => panic!("expected Error effect, got {other:?}"),
    };
    assert!(
        matches!(err, HandshakeError::Protocol(_)),
        "expected Protocol error for non-string result, got {err:?}"
    );
}

// ─── Stray-event skipping (D6) ───────────────────────────────────────────────

/// Stray events (wrong pubkey, undecryptable content) must produce empty
/// Vec<Effect> — the state machine does not advance and does not error.
/// The genuine response that arrives afterward must still complete the step.
#[test]
fn stray_events_are_skipped_then_genuine_response_completes() {
    let client_keys = Keys::generate();
    let bunker_keys = Keys::generate();
    let stranger = Keys::generate();
    let user_keys = Keys::generate();

    let (mut state, effects) = bunker_start(&client_keys, bunker_keys.public_key(), None, None);
    let connect_frame = match &effects[2] {
        Effect::SendFrame { text, .. } => text.clone(),
        other => panic!("expected SendFrame, got {other:?}"),
    };

    // Stray 1: event from a stranger (wrong pubkey).
    let stray = make_response_event(
        &stranger,
        client_keys.public_key(),
        json!({"id": "noise", "result": "ignored"}),
    );
    let skip_effects = state.on_relay_event(&stray, TEST_NOW);
    assert!(
        skip_effects.is_empty(),
        "stray event from stranger must be skipped: {skip_effects:?}"
    );

    // Stray 2: event with garbage ciphertext.
    let mut garbage = make_response_event(
        &bunker_keys,
        client_keys.public_key(),
        json!({"id": "noise2", "result": "x"}),
    );
    garbage["content"] = json!("not-real-ciphertext");
    let skip_effects2 = state.on_relay_event(&garbage, TEST_NOW);
    assert!(
        skip_effects2.is_empty(),
        "garbage ciphertext must be skipped"
    );

    // Now the genuine response arrives.
    let ack_event = respond_to_frame(
        &connect_frame,
        &bunker_keys,
        client_keys.public_key(),
        "ack",
    );
    let effects2 = state.on_relay_event(&ack_event, TEST_NOW);
    assert!(
        effects2
            .iter()
            .any(|e| matches!(e, Effect::Progress { stage, .. } if stage == "awaiting_pubkey")),
        "genuine connect response must advance to awaiting_pubkey"
    );

    // Continue with gpk.
    let gpk_frame = match effects2
        .iter()
        .find(|e| matches!(e, Effect::SendFrame { .. }))
    {
        Some(Effect::SendFrame { text, .. }) => text.clone(),
        _ => panic!("expected gpk SendFrame"),
    };
    let gpk_event = respond_to_frame(
        &gpk_frame,
        &bunker_keys,
        client_keys.public_key(),
        &user_keys.public_key().to_hex(),
    );
    let effects3 = state.on_relay_event(&gpk_event, TEST_NOW);
    assert!(
        matches!(effects3.first(), Some(Effect::SignerReady(_))),
        "must complete with SignerReady"
    );
}

/// Events processed after Done phase must produce empty effects.
#[test]
fn events_after_done_phase_are_silently_ignored() {
    let client_keys = Keys::generate();
    let bunker_keys = Keys::generate();
    let user_keys = Keys::generate();

    let (mut state, effects) = bunker_start(&client_keys, bunker_keys.public_key(), None, None);
    let connect_frame = match &effects[2] {
        Effect::SendFrame { text, .. } => text.clone(),
        _ => panic!(),
    };
    let ack = respond_to_frame(
        &connect_frame,
        &bunker_keys,
        client_keys.public_key(),
        "ack",
    );
    let eff2 = state.on_relay_event(&ack, TEST_NOW);
    let gpk_frame = match eff2.iter().find(|e| matches!(e, Effect::SendFrame { .. })) {
        Some(Effect::SendFrame { text, .. }) => text.clone(),
        _ => panic!(),
    };
    let gpk_resp = respond_to_frame(
        &gpk_frame,
        &bunker_keys,
        client_keys.public_key(),
        &user_keys.public_key().to_hex(),
    );
    let _ = state.on_relay_event(&gpk_resp, TEST_NOW); // → Done

    // Any further event must be a no-op.
    let noise = make_response_event(
        &bunker_keys,
        client_keys.public_key(),
        json!({"id": "late", "result": "x"}),
    );
    let post_done = state.on_relay_event(&noise, TEST_NOW);
    assert!(post_done.is_empty(), "Done phase must ignore all events");
}

// ─── Deadline / tick ─────────────────────────────────────────────────────────

/// `tick` must emit a `Timeout` error when called after the deadline.
#[test]
fn tick_after_deadline_emits_timeout_error() {
    let client_keys = Keys::generate();
    let bunker_pk = Keys::generate().public_key();
    let (mut state, _effects) = bunker_start(&client_keys, bunker_pk, None, None);

    // Before deadline: no effects.
    let pre = state.tick(TEST_NOW + 1);
    assert!(pre.is_empty(), "tick before deadline must be empty");

    // At or after deadline: Timeout error.
    let at_deadline = state.tick(TEST_NOW + 60);
    let err = match at_deadline.into_iter().next() {
        Some(Effect::Error { error }) => error,
        other => panic!("expected Timeout Error, got {other:?}"),
    };
    assert!(
        matches!(&err, HandshakeError::Timeout(msg) if msg.contains("connect") && msg.contains("60s")),
        "timeout message must name the step and budget: {err:?}"
    );
}

/// The step deadline must be (re-)armable to a fresh `now + 60s` AFTER the
/// driver has connected + subscribed — the relay dial (up to 10s) must NOT eat
/// into the 60s response budget. `start_bunker` carries an initial deadline,
/// but `arm_deadline` is what the driver calls post-subscribe; this asserts the
/// clock restarts from the supplied `now`, not from `start_bunker`'s timestamp.
#[test]
fn arm_deadline_restarts_budget_from_post_subscribe_now() {
    let client_keys = Keys::generate();
    let bunker_pk = Keys::generate().public_key();
    let (mut state, _effects) = bunker_start(&client_keys, bunker_pk, None, None);

    // start_bunker armed the deadline at start time.
    assert_eq!(state.deadline_at(), TEST_NOW + 60);

    // Simulate a 9s relay dial: the driver arms the deadline only after
    // connect + subscribe, so the full 60s budget starts from THERE.
    let post_subscribe_now = TEST_NOW + 9;
    state.arm_deadline(post_subscribe_now);
    assert_eq!(
        state.deadline_at(),
        post_subscribe_now + 60,
        "deadline must restart from post-subscribe now, not start time"
    );

    // A tick at the OLD deadline (TEST_NOW+60) must now be a no-op — the budget
    // was correctly extended past the dial time.
    let at_old_deadline = state.tick(TEST_NOW + 60);
    assert!(
        at_old_deadline.is_empty(),
        "the dial time must not count against the response budget"
    );
}

// ─── on_relay_text sub-id filtering ──────────────────────────────────────────

/// `on_relay_text` must only act on frames carrying THIS session's sub id; a
/// frame for a different subscription (multiplexed on the same socket) must be
/// ignored. Matters for the step-3 browser caller that forwards raw socket text.
#[test]
fn on_relay_text_ignores_frames_for_other_subscriptions() {
    let client_keys = Keys::generate();
    let bunker_keys = Keys::generate();
    let (mut state, effects) = bunker_start(&client_keys, bunker_keys.public_key(), None, None);
    let connect_frame = match &effects[2] {
        Effect::SendFrame { text, .. } => text.clone(),
        other => panic!("expected SendFrame, got {other:?}"),
    };

    // A genuine, decryptable connect ack — but wrapped in an EVENT frame whose
    // sub id is NOT this session's. It must be ignored despite valid content.
    let ack_event = respond_to_frame(
        &connect_frame,
        &bunker_keys,
        client_keys.public_key(),
        "ack",
    );
    let wrong_sub_frame =
        json!(["EVENT", "some-other-subscription", ack_event.clone()]).to_string();
    let ignored = state.on_relay_text(&wrong_sub_frame, TEST_NOW);
    assert!(
        ignored.is_empty(),
        "a frame for a different sub id must be ignored: {ignored:?}"
    );

    // The SAME event on THIS session's sub id advances the handshake.
    let right_sub_frame = json!(["EVENT", SUB_ID, ack_event]).to_string();
    let accepted = state.on_relay_text(&right_sub_frame, TEST_NOW);
    assert!(
        accepted
            .iter()
            .any(|e| matches!(e, Effect::SendFrame { .. })),
        "the matching sub id must advance to get_public_key: {accepted:?}"
    );
}
