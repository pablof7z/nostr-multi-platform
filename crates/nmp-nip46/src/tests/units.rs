//! Pure-function unit tests: `build_connect_params`, `new_request_id`,
//! `build_req_frame`, `decode_inbound_response`, and `build_event_frame`.

use nostr::nips::nip44;
use serde_json::Value;

use super::*;
use crate::build_req_frame;
use crate::rpc::{
    build_connect_params, build_event_frame, decode_inbound_response, new_request_id,
};

// ─── build_connect_params ────────────────────────────────────────────────────

#[test]
fn build_connect_params_emits_three_tuple_with_empties_when_absent() {
    let remote = Keys::generate().public_key();
    let params = build_connect_params(remote, None, None);
    let arr = params.as_array().expect("params is a JSON array");
    assert_eq!(arr.len(), 3, "NIP-46 connect params is a 3-tuple");
    assert_eq!(arr[0].as_str(), Some(remote.to_hex().as_str()));
    assert_eq!(arr[1].as_str(), Some(""), "absent secret -> empty string");
    assert_eq!(arr[2].as_str(), Some(""), "absent perms -> empty string");
}

#[test]
fn build_connect_params_includes_secret_and_perms_when_present() {
    let remote = Keys::generate().public_key();
    let params = build_connect_params(remote, Some("s3cr3t"), Some("sign_event:1"));
    let arr = params.as_array().unwrap();
    assert_eq!(arr[0].as_str(), Some(remote.to_hex().as_str()));
    assert_eq!(arr[1].as_str(), Some("s3cr3t"));
    assert_eq!(arr[2].as_str(), Some("sign_event:1"));
}

// ─── new_request_id ──────────────────────────────────────────────────────────

#[test]
fn new_request_id_is_eleven_char_lowercase_hex() {
    let id = new_request_id();
    assert_eq!(id.len(), 11, "request id is 11 chars wide");
    assert!(
        id.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "request id must be lowercase hex: {id:?}"
    );
}

#[test]
fn new_request_id_is_unique_across_calls() {
    // The counter advances every call, so a small batch must be distinct.
    let ids: std::collections::HashSet<String> = (0..64).map(|_| new_request_id()).collect();
    assert_eq!(ids.len(), 64, "request ids must not collide");
}

// ─── build_req_frame ─────────────────────────────────────────────────────────

#[test]
fn build_req_frame_subscribes_to_kind_24133_for_local_pubkey() {
    let pk = Keys::generate().public_key().to_hex();
    let frame = build_req_frame("sub-1", &pk, TEST_NOW);
    let v: Value = serde_json::from_str(&frame).expect("REQ frame is JSON");
    let arr = v.as_array().unwrap();
    assert_eq!(arr[0].as_str(), Some("REQ"));
    assert_eq!(arr[1].as_str(), Some("sub-1"));
    let filter = &arr[2];
    let kinds = filter.get("kinds").and_then(|k| k.as_array()).unwrap();
    assert_eq!(kinds.len(), 1);
    assert_eq!(kinds[0].as_u64(), Some(24133));
    let p_tag = filter.get("#p").and_then(|p| p.as_array()).unwrap();
    assert_eq!(p_tag[0].as_str(), Some(pk.as_str()));
}

#[test]
fn build_req_frame_since_is_now_minus_30() {
    let pk = Keys::generate().public_key().to_hex();
    let now = TEST_NOW;
    let frame = build_req_frame("sub-1", &pk, now);
    let v: Value = serde_json::from_str(&frame).unwrap();
    let since = v.as_array().unwrap()[2]
        .get("since")
        .and_then(|s| s.as_u64())
        .expect("since is a number");
    assert_eq!(since, now - 30, "since must be exactly now - 30");
}

#[test]
fn build_req_frame_since_saturates_at_zero_for_small_now() {
    let pk = Keys::generate().public_key().to_hex();
    // now = 5, so now - 30 would underflow; saturating_sub clamps to 0.
    let frame = build_req_frame("sub-1", &pk, 5);
    let v: Value = serde_json::from_str(&frame).unwrap();
    let since = v.as_array().unwrap()[2]
        .get("since")
        .and_then(|s| s.as_u64())
        .expect("since is a number");
    assert_eq!(since, 0, "since must saturate at 0, not overflow");
}

// ─── decode_inbound_response ─────────────────────────────────────────────────

#[test]
fn decode_inbound_response_returns_plaintext_for_matching_pubkey() {
    let client = Keys::generate();
    let bunker = Keys::generate();
    let rpc = serde_json::json!({"id": "x1", "result": "ack"});
    let event = make_response_event(&bunker, client.public_key(), rpc);
    let plaintext = decode_inbound_response(&event, &client, bunker.public_key())
        .expect("decodes a well-formed response");
    let parsed: Value = serde_json::from_str(&plaintext).unwrap();
    assert_eq!(parsed.get("result").and_then(|v| v.as_str()), Some("ack"));
}

#[test]
fn decode_inbound_response_rejects_event_from_other_pubkey() {
    let client = Keys::generate();
    let bunker = Keys::generate();
    let stranger = Keys::generate();
    let rpc = serde_json::json!({"id": "x1", "result": "ack"});
    // Event is genuinely from `stranger`, but we ask to decode it as if
    // it were from `bunker` — must return None, never panic (D6).
    let event = make_response_event(&stranger, client.public_key(), rpc);
    assert!(decode_inbound_response(&event, &client, bunker.public_key()).is_none());
}

#[test]
fn decode_inbound_response_returns_none_for_missing_content() {
    let client = Keys::generate();
    let bunker = Keys::generate();
    let event = serde_json::json!({
        "pubkey": bunker.public_key().to_hex(),
        "kind": 24133,
    });
    // No `content` field — must be None, no panic (D6).
    assert!(decode_inbound_response(&event, &client, bunker.public_key()).is_none());
}

#[test]
fn decode_inbound_response_returns_none_for_garbage_ciphertext() {
    let client = Keys::generate();
    let bunker = Keys::generate();
    let event = serde_json::json!({
        "pubkey": bunker.public_key().to_hex(),
        "kind": 24133,
        "content": "this-is-not-valid-nip44-ciphertext",
    });
    // Undecryptable content — must be None, no panic (D6).
    assert!(decode_inbound_response(&event, &client, bunker.public_key()).is_none());
}

// ─── build_event_frame ───────────────────────────────────────────────────────

/// The single-source wire builder must produce a NIP-01 `["EVENT", <event>]`
/// frame where the inner event has kind=24133 and a `["p", remote_hex]` tag.
/// This is the shared shape assertion that was previously duplicated between
/// `publish_rpc` in `handshake.rs` and `send_rpc` in `transport.rs`.
#[test]
fn build_event_frame_emits_kind_24133_with_p_tag_and_nip44_content() {
    let local = Keys::generate();
    let remote = Keys::generate().public_key();
    let plaintext = r#"{"id":"abc","method":"sign_event","params":[]}"#;

    let frame = build_event_frame(&local, remote, plaintext).expect("build succeeds");

    // Frame is a NIP-01 EVENT envelope.
    assert!(
        frame.starts_with("[\"EVENT\","),
        "frame must be an EVENT envelope: {frame:.80}"
    );

    let parsed: Value = serde_json::from_str(&frame).unwrap();
    let inner = &parsed.as_array().unwrap()[1];

    // kind = 24133
    assert_eq!(
        inner.get("kind").and_then(|v| v.as_u64()),
        Some(24133),
        "event must be kind 24133"
    );

    // single ["p", remote_hex] tag
    let tags = inner.get("tags").and_then(|v| v.as_array()).unwrap();
    assert!(
        tags.iter().any(|t| t.as_array().is_some_and(|a| {
            a.first().and_then(|v| v.as_str()) == Some("p")
                && a.get(1).and_then(|v| v.as_str()) == Some(remote.to_hex().as_str())
        })),
        "event must have a [\"p\", remote_hex] tag"
    );

    // content is NIP-44-decryptable by remote's key
    let ciphertext = inner.get("content").and_then(|v| v.as_str()).unwrap();
    let decrypted = nip44::decrypt(local.secret_key(), &remote, ciphertext.as_bytes())
        .expect("content must be NIP-44 decryptable by remote");
    assert_eq!(
        decrypted, plaintext,
        "decrypted content must equal plaintext"
    );
}
