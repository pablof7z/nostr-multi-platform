//! Handshake state-machine tests, split by flow:
//! - [`bunker`] — the client-initiated `bunker://` handshake + its
//!   `await_response` error/robustness paths;
//! - [`nostrconnect`] — the signer-initiated `nostrconnect://` handshake;
//! - [`units`] — pure-function units (`build_connect_params`,
//!   `new_request_id`, `build_req_frame`, `decode_inbound_response`).
//!
//! Shared test doubles + fixtures live here so every group reuses them via
//! `use super::*`.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crossbeam_channel::Receiver;
use nostr::nips::nip44;
use nostr::{Keys, PublicKey};
use serde_json::{json, Value};

use crate::relay_client::{RelayClient, RelayError};

mod bunker;
mod nostrconnect;
mod units;

/// A never-cancelled cancel receiver for the happy/error-path tests: the
/// returned `Sender` is leaked so the channel never disconnects, so the
/// handshake `select!` only ever fires on inbound events or the deadline.
fn never_cancel() -> Receiver<()> {
    let (tx, rx) = crossbeam_channel::unbounded::<()>();
    // Keep the sender alive for the whole process so `cancel_rx` never sees a
    // spurious `Disconnected` (which the handshake treats as cancellation).
    std::mem::forget(tx);
    rx
}

/// Test double for `RelayClient`. Every published frame is both retained
/// in `sent` (for post-hoc assertions on the main thread) and forwarded
/// over a notification channel so driver threads can *block* on the next
/// frame instead of polling — satisfying the D8 "no polling — ever"
/// doctrine in test code as well as production.
struct StubRelay {
    sent: Mutex<Vec<String>>,
    frame_tx: mpsc::Sender<String>,
}

impl StubRelay {
    /// Returns the relay plus a `frame_rx` that yields each outgoing
    /// frame as it is published. Driver threads take ownership of
    /// `frame_rx` and `recv()` on it; when the test drops its `Arc`
    /// to the relay, `frame_tx` drops, `recv()` returns `Disconnected`,
    /// and the driver exits — no cancel flag or poll loop required.
    fn new() -> (Arc<Self>, mpsc::Receiver<String>) {
        let (frame_tx, frame_rx) = mpsc::channel();
        (
            Arc::new(Self {
                sent: Mutex::new(Vec::new()),
                frame_tx,
            }),
            frame_rx,
        )
    }

    fn last_event(&self) -> Option<String> {
        self.sent.lock().unwrap().last().cloned()
    }
}

impl RelayClient for StubRelay {
    fn send(&self, frame: String) -> Result<(), RelayError> {
        self.sent.lock().unwrap().push(frame.clone());
        // Best-effort: if the driver has already exited (receiver
        // dropped) the send fails harmlessly — the test is winding down.
        let _ = self.frame_tx.send(frame);
        Ok(())
    }
    fn shutdown(&self) {}
}

/// Helper: simulate the relay echoing a bunker response. Takes the raw
/// outgoing client frame ("EVENT" with the encrypted request), extracts
/// the request id by decrypting it with the BUNKER's keys (we play both
/// sides in this test), and produces an encrypted response event.
fn bunker_response(
    outgoing_frame: &str,
    bunker_keys: &Keys,
    client_pubkey: PublicKey,
    result: &str,
) -> Value {
    // Parse the EVENT frame to extract the kind:24133 envelope.
    let parsed: Value = serde_json::from_str(outgoing_frame).unwrap();
    let event = &parsed.as_array().unwrap()[1];
    let ciphertext = event.get("content").and_then(|v| v.as_str()).unwrap();
    let plaintext = nip44::decrypt(
        bunker_keys.secret_key(),
        &client_pubkey,
        ciphertext.as_bytes(),
    )
    .unwrap();
    let rpc: Value = serde_json::from_str(&plaintext).unwrap();
    let request_id = rpc.get("id").and_then(|v| v.as_str()).unwrap();
    let response_json = json!({
        "id": request_id,
        "result": result,
    })
    .to_string();
    let response_ct = nip44::encrypt(
        bunker_keys.secret_key(),
        &client_pubkey,
        response_json.as_bytes(),
        nip44::Version::V2,
    )
    .unwrap();
    json!({
        "id": "deadbeef",
        "pubkey": bunker_keys.public_key().to_hex(),
        "created_at": 0,
        "kind": 24133,
        "tags": [["p", client_pubkey.to_hex()]],
        "content": response_ct,
        "sig": "00",
    })
}

/// Helper: manufacture an encrypted kind:24133 response event with an
/// arbitrary RPC payload (used to exercise error / malformed paths).
fn make_response_event(bunker_keys: &Keys, client_pubkey: PublicKey, rpc: Value) -> Value {
    let ciphertext = nip44::encrypt(
        bunker_keys.secret_key(),
        &client_pubkey,
        rpc.to_string().as_bytes(),
        nip44::Version::V2,
    )
    .unwrap();
    json!({
        "id": "deadbeef",
        "pubkey": bunker_keys.public_key().to_hex(),
        "created_at": 0,
        "kind": 24133,
        "tags": [["p", client_pubkey.to_hex()]],
        "content": ciphertext,
        "sig": "00",
    })
}

/// Helper: build the signer's `connect` event for the nostrconnect flow.
fn signer_connect_event(signer_keys: &Keys, client_pubkey: PublicKey, secret: &str) -> Value {
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
