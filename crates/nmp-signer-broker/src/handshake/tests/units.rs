//! Pure-function unit tests: `build_connect_params`, `new_request_id`,
//! `build_req_frame`, and `decode_inbound_response`.

use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::handshake::{
    build_connect_params, build_req_frame, decode_inbound_response, new_request_id,
};

// ─── build_connect_params ────────────────────────────────────────────

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

// ─── new_request_id ──────────────────────────────────────────────────

#[test]
fn new_request_id_is_eleven_char_lowercase_hex() {
    let id = new_request_id();
    assert_eq!(id.len(), 11, "request id is 11 chars wide");
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "request id must be lowercase hex: {id:?}"
    );
}

#[test]
fn new_request_id_is_unique_across_calls() {
    // The counter advances every call, so a small batch must be distinct.
    let ids: std::collections::HashSet<String> =
        (0..64).map(|_| new_request_id()).collect();
    assert_eq!(ids.len(), 64, "request ids must not collide");
}

// ─── build_req_frame ─────────────────────────────────────────────────

#[test]
fn build_req_frame_subscribes_to_kind_24133_for_local_pubkey() {
    let pk = Keys::generate().public_key().to_hex();
    let frame = build_req_frame("sub-1", &pk);
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
fn build_req_frame_since_is_recent_and_in_the_past() {
    let pk = Keys::generate().public_key().to_hex();
    let frame = build_req_frame("sub-1", &pk);
    let v: Value = serde_json::from_str(&frame).unwrap();
    let since = v.as_array().unwrap()[2]
        .get("since")
        .and_then(|s| s.as_u64())
        .expect("since is a number");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // since == now - 30s (with small slack for test execution time).
    assert!(since <= now, "since must not be in the future");
    assert!(
        now - since <= 35,
        "since should be ~30s in the past, got {}s",
        now - since
    );
}

// ─── decode_inbound_response ─────────────────────────────────────────

#[test]
fn decode_inbound_response_returns_plaintext_for_matching_pubkey() {
    let client = Keys::generate();
    let bunker = Keys::generate();
    let rpc = json!({"id": "x1", "result": "ack"});
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
    let rpc = json!({"id": "x1", "result": "ack"});
    // Event is genuinely from `stranger`, but we ask to decode it as if
    // it were from `bunker` — must return None, never panic (D6).
    let event = make_response_event(&stranger, client.public_key(), rpc);
    assert!(decode_inbound_response(&event, &client, bunker.public_key()).is_none());
}

#[test]
fn decode_inbound_response_returns_none_for_missing_content() {
    let client = Keys::generate();
    let bunker = Keys::generate();
    let event = json!({
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
    let event = json!({
        "pubkey": bunker.public_key().to_hex(),
        "kind": 24133,
        "content": "this-is-not-valid-nip44-ciphertext",
    });
    // Undecryptable content — must be None, no panic (D6).
    assert!(decode_inbound_response(&event, &client, bunker.public_key()).is_none());
}
