//! Handshake state-machine tests, split by flow:
//! - [`bunker`] — the client-initiated `bunker://` handshake reducer;
//! - [`nostrconnect`] — the signer-initiated `nostrconnect://` handshake reducer;
//! - [`units`] — pure-function units (`build_connect_params`,
//!   `new_request_id`, `build_req_frame`, `decode_inbound_response`,
//!   `build_event_frame`).
//! - [`uri_encode`] — percent-encoding unit tests.
//!
//! Shared test fixtures live here so every group reuses them via
//! `use super::*`.
//!
//! These are SYNCHRONOUS reducer tests — no threads, no channels, no blocking.
//! Each test builds events, feeds them to the reducer, and asserts effects.

use nostr::nips::nip44;
use nostr::{Keys, PublicKey};
use serde_json::{json, Value};

mod bunker;
mod nostrconnect;
mod units;
mod uri_encode;

/// A fixed "now" timestamp used throughout reducer tests so results are
/// deterministic.  We use a value deep in the past so `since` filters in REQ
/// frames don't accidentally set a future timestamp.
pub(super) const TEST_NOW: u64 = 1_700_000_000;

// ─── Shared event-building helpers ───────────────────────────────────────────

/// Build a simulated bunker response event.
///
/// Encrypts `{id: request_id, result: result_str}` with `signer_keys`
/// addressed to `client_pubkey`, producing a fake kind:24133 inbound event
/// the reducer will accept as a valid response.
pub(super) fn make_response_event(
    signer_keys: &Keys,
    client_pubkey: PublicKey,
    rpc: Value,
) -> Value {
    let ciphertext = nip44::encrypt(
        signer_keys.secret_key(),
        &client_pubkey,
        rpc.to_string().as_bytes(),
        nip44::Version::V2,
    )
    .unwrap();
    json!({
        "id": "deadbeef",
        "pubkey": signer_keys.public_key().to_hex(),
        "created_at": 0,
        "kind": 24133,
        "tags": [["p", client_pubkey.to_hex()]],
        "content": ciphertext,
        "sig": "00",
    })
}

/// Build a response event by extracting the request id from the outgoing
/// client frame, then replying with `result_str`.
///
/// `outgoing_frame` is one of the `SendFrame.text` strings the reducer
/// emitted; `responder_keys` plays the role of the remote signer.
pub(super) fn respond_to_frame(
    outgoing_frame: &str,
    responder_keys: &Keys,
    client_pubkey: PublicKey,
    result_str: &str,
) -> Value {
    // Decrypt the outgoing EVENT frame to extract the request id.
    let parsed: Value = serde_json::from_str(outgoing_frame).unwrap();
    let event = &parsed.as_array().unwrap()[1];
    let ciphertext = event.get("content").and_then(|v| v.as_str()).unwrap();
    let plaintext = nip44::decrypt(
        responder_keys.secret_key(),
        &client_pubkey,
        ciphertext.as_bytes(),
    )
    .unwrap();
    let rpc: Value = serde_json::from_str(&plaintext).unwrap();
    let request_id = rpc.get("id").and_then(|v| v.as_str()).unwrap();
    let response_rpc = json!({ "id": request_id, "result": result_str });
    make_response_event(responder_keys, client_pubkey, response_rpc)
}

/// Build the signer's `connect` event for the nostrconnect flow.
///
/// The signer encrypts a `{method:"connect", params:[signer_pubkey, secret]}`
/// RPC to the client's public key.
pub(super) fn signer_connect_event(
    signer_keys: &Keys,
    client_pubkey: PublicKey,
    secret: &str,
) -> Value {
    let rpc = json!({
        "id": "conn-1",
        "method": "connect",
        "params": [signer_keys.public_key().to_hex(), secret],
    });
    let ct = nip44::encrypt(
        signer_keys.secret_key(),
        &client_pubkey,
        rpc.to_string().as_bytes(),
        nip44::Version::V2,
    )
    .unwrap();
    json!({
        "id": "deadbeef",
        "pubkey": signer_keys.public_key().to_hex(),
        "created_at": 0,
        "kind": 24133,
        "tags": [["p", client_pubkey.to_hex()]],
        "content": ct,
        "sig": "00",
    })
}

/// Decrypt the plaintext content of an outgoing SendFrame effect.
/// `responder_keys` is the party the frame was encrypted TO.
pub(super) fn decrypt_outgoing_frame(frame_text: &str, responder_keys: &Keys, client_pubkey: PublicKey) -> Value {
    let parsed: Value = serde_json::from_str(frame_text).unwrap();
    let event = &parsed.as_array().unwrap()[1];
    let ciphertext = event.get("content").and_then(|v| v.as_str()).unwrap();
    let plaintext = nip44::decrypt(
        responder_keys.secret_key(),
        &client_pubkey,
        ciphertext.as_bytes(),
    )
    .unwrap();
    serde_json::from_str(&plaintext).unwrap()
}
