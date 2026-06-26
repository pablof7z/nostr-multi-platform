//! Synchronous reducer tests for the signer-initiated (`nostrconnect://`)
//! handshake.
//!
//! No threads, no channels, no blocking — each test feeds events directly to
//! the reducer and asserts the returned effects.

use nostr::Keys;
use serde_json::{json, Value};

use super::*;
use crate::effect::Effect;
use crate::error::HandshakeError;
use crate::nostrconnect::start_nostrconnect;

const SUB_ID: &str = "nmp-bunker";

/// Helper: start a nostrconnect session with a fixed relay + secret.
fn nc_start(
    local_keys: &Keys,
    secret: &str,
    perms: Option<String>,
) -> (String, crate::reducer::SessionState, Vec<Effect>) {
    start_nostrconnect(
        SUB_ID,
        local_keys.clone(),
        "wss://relay.example.com".to_string(),
        secret.to_string(),
        perms,
        "nmp",
        TEST_NOW,
    )
}

// ─── URI shape ───────────────────────────────────────────────────────────────

#[test]
fn start_nostrconnect_returns_well_formed_uri() {
    let local_keys = Keys::generate();
    let (uri, _, _) = nc_start(&local_keys, "testsecret", None);

    assert!(uri.starts_with("nostrconnect://"), "uri must use nostrconnect scheme: {uri:?}");
    let (pubkey_hex, _query) = uri
        .strip_prefix("nostrconnect://")
        .unwrap()
        .split_once('?')
        .expect("must have query string");
    assert_eq!(pubkey_hex.len(), 64, "pubkey must be 64 hex chars");
    assert!(uri.contains("secret=testsecret"), "uri must embed secret: {uri:?}");
    assert!(uri.contains("name=nmp"), "uri must embed name: {uri:?}");
}

#[test]
fn start_nostrconnect_encodes_perms_in_uri() {
    let local_keys = Keys::generate();
    let (uri, _, _) =
        nc_start(&local_keys, "s", Some("sign_event:1,sign_event:7".to_string()));
    assert!(
        uri.contains("perms=sign_event%3A1%2Csign_event%3A7"),
        "perms must be percent-encoded: {uri:?}"
    );
}

#[test]
fn start_nostrconnect_omits_perms_when_none() {
    let local_keys = Keys::generate();
    let (uri, _, _) = nc_start(&local_keys, "s", None);
    assert!(!uri.contains("perms="), "perms must be absent when None: {uri:?}");
}

// ─── Initial effects ─────────────────────────────────────────────────────────

#[test]
fn start_nostrconnect_emits_subscribe_and_connecting_progress() {
    let local_keys = Keys::generate();
    let (_, _, effects) = nc_start(&local_keys, "s", None);

    assert!(matches!(&effects[0], Effect::Subscribe { .. }), "first effect must be Subscribe");
    assert!(
        matches!(&effects[1], Effect::Progress { stage, .. } if stage == "connecting"),
        "second effect must be connecting progress"
    );
    assert_eq!(effects.len(), 2, "only Subscribe + Progress on start");
}

// ─── Security: wrong secret ───────────────────────────────────────────────────

/// Security-critical: a `connect` frame whose `params[1]` secret does not
/// match the expected session secret must be rejected with a definitive
/// `BunkerError`, never accepted.
#[test]
fn wrong_secret_produces_bunker_error() {
    let local_keys = Keys::generate();
    let signer_keys = Keys::generate();
    let (_, mut state, _) = nc_start(&local_keys, "the-real-secret", None);

    // Signer sends connect with WRONG secret.
    let bad = signer_connect_event(&signer_keys, local_keys.public_key(), "wrong-secret");
    let effects = state.on_relay_event(&bad, TEST_NOW);

    let err = match effects.into_iter().next() {
        Some(Effect::Error { error }) => error,
        other => panic!("expected Error effect, got {other:?}"),
    };
    match err {
        HandshakeError::BunkerError(msg) => {
            assert!(msg.contains("secret mismatch"), "must report secret mismatch: {msg:?}");
        }
        other => panic!("expected BunkerError, got {other:?}"),
    }
}

// ─── Happy path ──────────────────────────────────────────────────────────────

/// Happy path: valid connect with correct secret, ACK + gpk send, then gpk
/// reply → SignerReady with the correct pubkeys.
#[test]
fn happy_path_returns_signer_ready_with_correct_pubkeys() {
    let local_keys = Keys::generate();
    let signer_keys = Keys::generate();
    let user_keys = Keys::generate();
    let secret = "session-secret-xyz";

    let (_, mut state, _) = nc_start(&local_keys, secret, None);

    // Step 1: signer sends connect with correct secret.
    let connect_event =
        signer_connect_event(&signer_keys, local_keys.public_key(), secret);
    let effects1 = state.on_relay_event(&connect_event, TEST_NOW);

    // Expect: SendFrame(ack), Progress("awaiting_pubkey"), SendFrame(get_public_key)
    assert!(
        matches!(&effects1[0], Effect::SendFrame { .. }),
        "first effect must be ACK SendFrame: {effects1:?}"
    );
    assert!(
        matches!(&effects1[1], Effect::Progress { stage, .. } if stage == "awaiting_pubkey"),
        "second effect must be awaiting_pubkey progress: {effects1:?}"
    );
    let gpk_frame = match &effects1[2] {
        Effect::SendFrame { text, .. } => text.clone(),
        other => panic!("expected get_public_key SendFrame, got {other:?}"),
    };

    // Verify ACK content: {id: connect_id, result: "ack"} encrypted to signer.
    let ack_rpc =
        decrypt_outgoing_frame(&effects1[0].text(), &signer_keys, local_keys.public_key());
    assert_eq!(ack_rpc.get("result").and_then(|v| v.as_str()), Some("ack"), "ACK must be ack");
    assert_eq!(ack_rpc.get("id").and_then(|v| v.as_str()), Some("conn-1"), "ACK id must match connect id");

    // Step 2: signer replies to get_public_key.
    let user_pk_hex = user_keys.public_key().to_hex();
    let gpk_rpc: Value = decrypt_outgoing_frame(&gpk_frame, &signer_keys, local_keys.public_key());
    let gpk_id = gpk_rpc.get("id").and_then(|v| v.as_str()).unwrap();
    let gpk_resp_rpc = json!({ "id": gpk_id, "result": user_pk_hex });
    let gpk_event = make_response_event(&signer_keys, local_keys.public_key(), gpk_resp_rpc);
    let effects2 = state.on_relay_event(&gpk_event, TEST_NOW);

    let sr = match effects2.into_iter().next() {
        Some(Effect::SignerReady(sr)) => sr,
        other => panic!("expected SignerReady, got {other:?}"),
    };
    assert_eq!(sr.user_pubkey_hex, user_pk_hex, "user pubkey must match");
    assert_eq!(
        sr.remote_signer_pubkey_hex,
        signer_keys.public_key().to_hex(),
        "remote signer pubkey must match"
    );
}

// ─── Stray-event skipping (D6) ───────────────────────────────────────────────

/// Stray events (wrong decryption, not `connect` method) must be silently
/// skipped; a subsequent valid `connect` must still work.
#[test]
fn stray_events_skipped_then_valid_connect_completes() {
    let local_keys = Keys::generate();
    let signer_keys = Keys::generate();
    let stranger = Keys::generate();
    let user_keys = Keys::generate();
    let secret = "my-secret";

    let (_, mut state, _) = nc_start(&local_keys, secret, None);

    // Stray 1: content encrypted to a random THIRD party, not to local_keys —
    // the reducer tries decrypt(local_keys_sk, stranger_pk, ct) but the ct was
    // encrypted to a different recipient's pk, so ECDH yields the wrong shared
    // secret → decryption fails → event is silently skipped (D6).
    let decoy_recipient = nostr::Keys::generate();
    let bad_rpc = serde_json::json!({
        "id": "x",
        "method": "connect",
        "params": [stranger.public_key().to_hex(), secret],
    });
    let bad_ct = nostr::nips::nip44::encrypt(
        stranger.secret_key(),
        &decoy_recipient.public_key(), // encrypted to the WRONG recipient
        bad_rpc.to_string().as_bytes(),
        nostr::nips::nip44::Version::V2,
    )
    .unwrap();
    let noise = serde_json::json!({
        "pubkey": stranger.public_key().to_hex(),
        "content": bad_ct,
    });
    let skip1 = state.on_relay_event(&noise, TEST_NOW);
    assert!(skip1.is_empty(), "undecryptable event must be skipped");

    // Stray 2: valid decryption but wrong method.
    let wrong_method_rpc = json!({
        "id": "x",
        "method": "sign_event",
        "params": [signer_keys.public_key().to_hex(), secret],
    });
    let ct = nostr::nips::nip44::encrypt(
        signer_keys.secret_key(),
        &local_keys.public_key(),
        wrong_method_rpc.to_string().as_bytes(),
        nostr::nips::nip44::Version::V2,
    )
    .unwrap();
    let wrong_method_event = json!({
        "pubkey": signer_keys.public_key().to_hex(),
        "content": ct,
    });
    let skip2 = state.on_relay_event(&wrong_method_event, TEST_NOW);
    assert!(skip2.is_empty(), "non-connect method must be skipped");

    // Now the real connect arrives.
    let connect = signer_connect_event(&signer_keys, local_keys.public_key(), secret);
    let effects = state.on_relay_event(&connect, TEST_NOW);
    assert!(
        effects.iter().any(|e| matches!(e, Effect::Progress { stage, .. } if stage == "awaiting_pubkey")),
        "valid connect must advance state"
    );

    // Drive to completion.
    let gpk_frame = effects
        .iter()
        .filter_map(|e| if let Effect::SendFrame { text, .. } = e { Some(text.clone()) } else { None })
        .next_back()
        .expect("gpk SendFrame");
    let gpk_rpc = decrypt_outgoing_frame(&gpk_frame, &signer_keys, local_keys.public_key());
    let gpk_id = gpk_rpc.get("id").and_then(|v| v.as_str()).unwrap();
    let gpk_resp = make_response_event(
        &signer_keys,
        local_keys.public_key(),
        json!({ "id": gpk_id, "result": user_keys.public_key().to_hex() }),
    );
    let final_effects = state.on_relay_event(&gpk_resp, TEST_NOW);
    assert!(
        matches!(final_effects.first(), Some(Effect::SignerReady(_))),
        "must complete with SignerReady"
    );
}

// ─── Deadline / tick ─────────────────────────────────────────────────────────

#[test]
fn tick_after_deadline_emits_timeout_for_connect_frame() {
    let local_keys = Keys::generate();
    let (_, mut state, _) = nc_start(&local_keys, "s", None);

    let effects = state.tick(TEST_NOW + 60);
    let err = match effects.into_iter().next() {
        Some(Effect::Error { error }) => error,
        other => panic!("expected Timeout Error, got {other:?}"),
    };
    assert!(
        matches!(&err, HandshakeError::Timeout(msg) if msg.contains("connect frame from signer")),
        "timeout must name the nostrconnect step: {err:?}"
    );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

// Trait to simplify extracting `text` from Effect::SendFrame in tests.
trait EffectExt {
    fn text(&self) -> String;
}

impl EffectExt for Effect {
    fn text(&self) -> String {
        match self {
            Effect::SendFrame { text, .. } => text.clone(),
            other => panic!("expected SendFrame, got {other:?}"),
        }
    }
}
