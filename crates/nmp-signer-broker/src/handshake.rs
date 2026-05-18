//! NIP-46 handshake state machine.
//!
//! Pure-ish module: takes a `RelayClient` impl and a `Keys` (local ephemeral
//! key), runs the `connect` → `get_public_key` dance, returns the user's
//! pubkey. Side effects are limited to: publishing on the relay client,
//! receiving inbound events via a `Receiver<Value>` it sets up, and bumping
//! a cancellation flag.
//!
//! ## Protocol shape (client-initiated, the `bunker://` URI form)
//!
//! 1. **Subscribe** to kind:24133 events tagged with the local ephemeral
//!    pubkey via `#p`. Use `since = now - 30s` to avoid replaying ancient
//!    bunker-side responses.
//! 2. **connect**: send `{"id":<nanoid>,"method":"connect","params":[<remote_pubkey>,<secret_or_empty>,<perms_or_empty>]}`
//!    NIP-44-encrypted to the remote pubkey, wrapped in a kind:24133 event
//!    tagged `["p", <remote_pubkey>]`. Real bunkers reply with `result:"ack"`
//!    OR with the user pubkey OR with an empty string. Treat any non-error
//!    response as success.
//! 3. **get_public_key**: send `{"id":<nanoid>,"method":"get_public_key","params":[]}`
//!    same envelope. Response `result` is the user's pubkey hex.
//!
//! ## Why a separate function?
//!
//! Pulling the state machine out of `BunkerBroker` keeps `broker.rs` focused
//! on lifecycle / cancellation and lets us unit-test the protocol logic with
//! a `Vec`-backed `RelayClient` stub.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nostr::nips::nip44;
use nostr::{EventBuilder, Keys, Kind, PublicKey, Tag, Timestamp};
use serde_json::{json, Value};

use crate::relay_client::RelayClient;

/// Errors produced by the handshake state machine. Display strings flow
/// directly to `BunkerHandshakeProgress { stage: "failed", message }`.
#[derive(Debug, Clone)]
pub enum HandshakeError {
    /// Cancelled via `BunkerBroker::cancel`.
    Cancelled,
    /// Overall handshake deadline elapsed.
    Timeout(String),
    /// The bunker returned an explicit error response.
    BunkerError(String),
    /// Crypto / serialisation / parsing failure.
    Protocol(String),
    /// Relay write / transport error.
    Transport(String),
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandshakeError::Cancelled => f.write_str("cancelled"),
            HandshakeError::Timeout(s) => write!(f, "timeout: {s}"),
            HandshakeError::BunkerError(s) => write!(f, "bunker error: {s}"),
            HandshakeError::Protocol(s) => write!(f, "protocol error: {s}"),
            HandshakeError::Transport(s) => write!(f, "transport error: {s}"),
        }
    }
}

impl std::error::Error for HandshakeError {}

/// Result of a successful handshake: the user's pubkey hex.
#[derive(Debug, Clone)]
pub struct HandshakeOutcome {
    /// The user's pubkey, returned by `get_public_key`. This is what
    /// `RemoteSignerHandle::pubkey_hex` will report to the actor.
    pub user_pubkey_hex: String,
}

/// Per-handshake step deadline. The bunker often needs the user to tap
/// approve on the phone; ~60s covers normal UX.
const STEP_TIMEOUT: Duration = Duration::from_secs(60);

/// Build the REQ frame the broker uses to subscribe to inbound responses
/// addressed to `local_pubkey_hex`.
pub fn build_req_frame(sub_id: &str, local_pubkey_hex: &str) -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .saturating_sub(30);
    json!([
        "REQ",
        sub_id,
        {
            "kinds": [24133],
            "#p": [local_pubkey_hex],
            "since": since,
        }
    ])
    .to_string()
}

/// Run the handshake. Caller owns the relay client (already connected +
/// subscribed) and the inbound event receiver. Returns the user pubkey on
/// success.
///
/// `progress` is an `&mut dyn FnMut(&str, Option<&str>)` so the broker can
/// publish progress events to the actor. The handshake itself emits two
/// transitions: `"connecting"` (before `connect`) and `"awaiting_pubkey"`
/// (before `get_public_key`). The final `"ready"` is emitted by the broker
/// after constructing the signer.
#[allow(clippy::too_many_arguments)] // protocol state machine — eight closely related inputs
pub fn run_handshake(
    relay: &dyn RelayClient,
    inbound_rx: &Receiver<Value>,
    local_keys: &Keys,
    remote_pubkey: PublicKey,
    secret: Option<&str>,
    perms: Option<&str>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&str, Option<&str>),
) -> Result<HandshakeOutcome, HandshakeError> {
    // Step 1 — connect.
    progress("connecting", Some("Sending connect to bunker"));
    let connect_params = build_connect_params(remote_pubkey, secret, perms);
    let connect_id = new_request_id();
    publish_rpc(
        relay,
        local_keys,
        remote_pubkey,
        &connect_id,
        "connect",
        connect_params,
    )?;
    // Treat any non-error response to `connect` as success; some bunkers
    // reply with `"ack"`, others with the user pubkey, others with an empty
    // string. The authoritative pubkey comes from `get_public_key` below.
    let _connect_resp = await_response(
        inbound_rx,
        &connect_id,
        local_keys,
        remote_pubkey,
        cancel,
        STEP_TIMEOUT,
        "connect",
    )?;

    // Step 2 — get_public_key.
    progress("awaiting_pubkey", Some("Awaiting bunker approval"));
    let gpk_id = new_request_id();
    publish_rpc(
        relay,
        local_keys,
        remote_pubkey,
        &gpk_id,
        "get_public_key",
        Value::Array(Vec::new()),
    )?;
    let gpk_resp = await_response(
        inbound_rx,
        &gpk_id,
        local_keys,
        remote_pubkey,
        cancel,
        STEP_TIMEOUT,
        "get_public_key",
    )?;
    let user_pubkey_hex = gpk_resp.trim();
    if user_pubkey_hex.len() != 64 || !user_pubkey_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(HandshakeError::Protocol(format!(
            "get_public_key returned non-hex: {user_pubkey_hex:?}"
        )));
    }
    Ok(HandshakeOutcome {
        user_pubkey_hex: user_pubkey_hex.to_ascii_lowercase(),
    })
}

/// Build the `connect` params list.
///
/// NIP-46 spec accepts either `[remote, secret]` or `[remote, secret, perms]`.
/// We always send the 3-tuple, with empty strings filling absent fields —
/// this is what most modern bunkers expect.
fn build_connect_params(remote: PublicKey, secret: Option<&str>, perms: Option<&str>) -> Value {
    json!([remote.to_hex(), secret.unwrap_or(""), perms.unwrap_or(""),])
}

/// Generate a request id (11-byte lowercase hex, mirroring the
/// `nmp-signers::mapper::generate_request_id` shape).
fn new_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering as AOrd};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, AOrd::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    format!(
        "{:011x}",
        (n.wrapping_mul(0x9E37_79B9) ^ nanos) & 0xFFFF_FFFF_FFFF
    )
}

/// Encrypt a JSON-RPC envelope, wrap as kind:24133, sign with local keys,
/// publish via the relay client.
fn publish_rpc(
    relay: &dyn RelayClient,
    local_keys: &Keys,
    remote_pubkey: PublicKey,
    id: &str,
    method: &str,
    params: Value,
) -> Result<(), HandshakeError> {
    let envelope = json!({
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string();
    let ciphertext = nip44::encrypt(
        local_keys.secret_key(),
        &remote_pubkey,
        envelope.as_bytes(),
        nip44::Version::V2,
    )
    .map_err(|e| HandshakeError::Protocol(format!("nip44 encrypt: {e}")))?;
    let event = EventBuilder::new(Kind::from_u16(24133), ciphertext)
        .tags(vec![Tag::parse(["p", &remote_pubkey.to_hex()]).map_err(
            |e| HandshakeError::Protocol(format!("tag parse: {e}")),
        )?])
        .custom_created_at(Timestamp::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        ))
        .sign_with_keys(local_keys)
        .map_err(|e| HandshakeError::Protocol(format!("sign event: {e}")))?;
    let serialized = serde_json::to_string(&event)
        .map_err(|e| HandshakeError::Protocol(format!("serialize event: {e}")))?;
    let frame = format!(r#"["EVENT",{serialized}]"#);
    relay
        .send(frame)
        .map_err(|e| HandshakeError::Transport(e.to_string()))
}

/// Block waiting for the response to `expected_id`. The receiver carries the
/// raw event JSON (the third element of `["EVENT", sub_id, event_json]`).
/// Each event is decrypted with NIP-44, parsed as JSON-RPC, and matched on
/// `id`. Other events (e.g. responses to other in-flight RPCs) are dropped.
fn await_response(
    inbound_rx: &Receiver<Value>,
    expected_id: &str,
    local_keys: &Keys,
    remote_pubkey: PublicKey,
    cancel: &AtomicBool,
    timeout: Duration,
    method_label: &str,
) -> Result<String, HandshakeError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(HandshakeError::Cancelled);
        }
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or_else(|| {
                HandshakeError::Timeout(format!("no response to {method_label} within {timeout:?}"))
            })?;
        let wait = remaining.min(Duration::from_millis(200));
        let event = match inbound_rx.recv_timeout(wait) {
            Ok(v) => v,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(HandshakeError::Transport(
                    "inbound channel disconnected".to_string(),
                ));
            }
        };
        let Some(ciphertext) = event.get("content").and_then(|v| v.as_str()) else {
            continue;
        };
        let event_pubkey = event.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
        if event_pubkey.to_ascii_lowercase() != remote_pubkey.to_hex() {
            // Stray event addressed to us from a different signer; ignore.
            continue;
        }
        let plaintext = match nip44::decrypt(
            local_keys.secret_key(),
            &remote_pubkey,
            ciphertext.as_bytes(),
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("nmp-signer-broker: nip44 decrypt failed: {e}");
                continue;
            }
        };
        let rpc: Value = match serde_json::from_str(&plaintext) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("nmp-signer-broker: rpc parse failed: {e}");
                continue;
            }
        };
        let id_match = rpc.get("id").and_then(|v| v.as_str()) == Some(expected_id);
        if !id_match {
            continue;
        }
        if let Some(err) = rpc.get("error") {
            if !err.is_null() {
                let msg = err
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| err.to_string());
                return Err(HandshakeError::BunkerError(msg));
            }
        }
        let result = rpc.get("result").and_then(|v| v.as_str()).ok_or_else(|| {
            HandshakeError::Protocol(format!("{method_label} response missing string result"))
        })?;
        return Ok(result.to_string());
    }
}

/// Steady-state inbound dispatcher used by `BrokerTransport`: parse a raw
/// kind:24133 event, decrypt the content with NIP-44, and return
/// `(id, result_or_error_json)` for the signer's `deliver_rpc_response`.
/// Returns `None` if the event is malformed or addressed to a different key.
pub fn decode_inbound_response(
    event: &Value,
    local_keys: &Keys,
    remote_pubkey: PublicKey,
) -> Option<String> {
    let ciphertext = event.get("content").and_then(|v| v.as_str())?;
    let event_pubkey = event.get("pubkey").and_then(|v| v.as_str())?;
    if event_pubkey.to_ascii_lowercase() != remote_pubkey.to_hex() {
        return None;
    }
    nip44::decrypt(
        local_keys.secret_key(),
        &remote_pubkey,
        ciphertext.as_bytes(),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay_client::{RelayClient, RelayError};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    struct StubRelay {
        sent: Mutex<Vec<String>>,
    }

    impl StubRelay {
        fn new() -> (Arc<Self>, mpsc::Receiver<Value>) {
            // The receiver half is returned only so the test can keep one
            // alive if it ever wants to inspect the worker's inbound side;
            // the stub does not actually push events through it.
            let (_tx, rx) = mpsc::channel();
            (
                Arc::new(Self {
                    sent: Mutex::new(Vec::new()),
                }),
                rx,
            )
        }

        fn last_event(&self) -> Option<String> {
            self.sent.lock().unwrap().last().cloned()
        }
    }

    impl RelayClient for StubRelay {
        fn send(&self, frame: String) -> Result<(), RelayError> {
            self.sent.lock().unwrap().push(frame);
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

    #[test]
    fn happy_path_connect_then_get_public_key_returns_user_pubkey() {
        let client_keys = Keys::generate();
        let bunker_keys = Keys::generate();
        let bunker_pubkey = bunker_keys.public_key();
        let user_keys = Keys::generate();
        let user_pk_hex = user_keys.public_key().to_hex();

        let (relay, _events_rx_dropped) = StubRelay::new();
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel);

        // Driver thread: snoop the relay's outgoing buffer, manufacture
        // responses, push them onto the inbound channel. We poll for new
        // outgoing frames.
        let relay_clone = Arc::clone(&relay);
        let bunker_keys_for_driver = bunker_keys.clone();
        let client_pk_for_driver = client_keys.public_key();
        let user_pk_for_driver = user_pk_hex.clone();
        let driver = std::thread::spawn(move || {
            let mut seen = 0usize;
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                if cancel_clone.load(Ordering::Relaxed) {
                    return;
                }
                let frames = relay_clone.sent.lock().unwrap().clone();
                if frames.len() > seen {
                    for (idx, frame) in frames[seen..].iter().enumerate() {
                        // Frame 0 is `connect` (reply "ack"); frame 1 is
                        // `get_public_key` (reply user pubkey).
                        let absolute_idx = seen + idx;
                        let result = if absolute_idx == 0 {
                            "ack".to_string()
                        } else {
                            user_pk_for_driver.clone()
                        };
                        let response = bunker_response(
                            frame,
                            &bunker_keys_for_driver,
                            client_pk_for_driver,
                            &result,
                        );
                        let _ = inbound_tx.send(response);
                    }
                    seen = frames.len();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        let mut progress_events: Vec<(String, Option<String>)> = Vec::new();
        let outcome = run_handshake(
            relay.as_ref(),
            &inbound_rx,
            &client_keys,
            bunker_pubkey,
            None,
            None,
            &cancel,
            &mut |stage, msg| progress_events.push((stage.to_string(), msg.map(String::from))),
        )
        .expect("handshake completes");

        assert_eq!(outcome.user_pubkey_hex, user_keys.public_key().to_hex());
        assert!(progress_events.iter().any(|(s, _)| s == "connecting"));
        assert!(progress_events.iter().any(|(s, _)| s == "awaiting_pubkey"));
        assert!(relay.last_event().is_some());

        // Wind the driver down.
        cancel.store(true, Ordering::Relaxed);
        let _ = driver.join();
    }

    #[test]
    fn cancellation_aborts_with_cancelled_error() {
        let client_keys = Keys::generate();
        let bunker_pk = Keys::generate().public_key();

        let (relay, _events) = StubRelay::new();
        let (_inbound_tx, inbound_rx) = mpsc::channel::<Value>();

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            cancel_clone.store(true, Ordering::Relaxed);
        });

        let err = run_handshake(
            relay.as_ref(),
            &inbound_rx,
            &client_keys,
            bunker_pk,
            None,
            None,
            &cancel,
            &mut |_, _| {},
        )
        .expect_err("cancelled");
        assert!(matches!(err, HandshakeError::Cancelled));
    }
}
