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
            Self::Cancelled => f.write_str("cancelled"),
            Self::Timeout(s) => write!(f, "timeout: {s}"),
            Self::BunkerError(s) => write!(f, "bunker error: {s}"),
            Self::Protocol(s) => write!(f, "protocol error: {s}"),
            Self::Transport(s) => write!(f, "transport error: {s}"),
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
#[must_use] 
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
        // D6: no stderr I/O from library code. A decrypt failure means this
        // event is not for us (or is malformed) — skip it silently.
        let Ok(plaintext) = nip44::decrypt(
            local_keys.secret_key(),
            &remote_pubkey,
            ciphertext.as_bytes(),
        ) else {
            continue;
        };
        // D6: a malformed RPC payload is skipped silently.
        let Ok(rpc) = serde_json::from_str::<Value>(&plaintext) else {
            continue;
        };
        let id_match = rpc.get("id").and_then(|v| v.as_str()) == Some(expected_id);
        if !id_match {
            continue;
        }
        if let Some(err) = rpc.get("error") {
            if !err.is_null() {
                let msg = err
                    .as_str().map_or_else(|| err.to_string(), str::to_string);
                return Err(HandshakeError::BunkerError(msg));
            }
        }
        let result = rpc.get("result").and_then(|v| v.as_str()).ok_or_else(|| {
            HandshakeError::Protocol(format!("{method_label} response missing string result"))
        })?;
        return Ok(result.to_string());
    }
}

// ─── nostrconnect:// (signer-initiated) handshake ────────────────────────────

/// Result of a successful nostrconnect:// handshake: the signer's pubkey and
/// the user's pubkey (as returned by `get_public_key`).
#[derive(Debug, Clone)]
pub struct NostrConnectOutcome {
    /// The remote signer's pubkey (learned from `event.pubkey` of the first
    /// inbound `connect` frame). Needed to construct the `BrokerTransport`.
    pub signer_pubkey_hex: String,
    /// The user pubkey returned by `get_public_key` — what
    /// `RemoteSignerHandle::pubkey_hex` will report to the actor.
    pub user_pubkey_hex: String,
}

/// Run the signer-initiated (`nostrconnect://`) handshake.
///
/// ## Protocol (signer-initiated)
///
/// 1. **Wait** for the first inbound kind:24133 whose decrypted `method` is
///    `"connect"` with `params = [<signer_pubkey>, <secret>, ...]`.
///    - Validate that `params[1] == expected_secret`. Reject otherwise.
///    - Learn `signer_pubkey = event.pubkey`.
/// 2. **Reply** `{id: <connect_id>, result: "ack"}` encrypted to the signer.
/// 3. **Send** `get_public_key` RPC to the signer.
/// 4. **Await** the `get_public_key` response; return the user pubkey.
///
/// `progress` emits: `"connecting"` (waiting for signer), `"awaiting_pubkey"`
/// (after ack, before `get_public_key` response), `"failed"` on error.
#[allow(clippy::too_many_arguments)]
pub fn run_nostrconnect_handshake(
    relay: &dyn RelayClient,
    inbound_rx: &Receiver<Value>,
    local_keys: &Keys,
    expected_secret: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&str, Option<&str>),
) -> Result<NostrConnectOutcome, HandshakeError> {
    // Step 1 — wait for the signer's connect event.
    progress("connecting", Some("Waiting for signer to scan QR code"));
    let (signer_pubkey, connect_id) = await_nostrconnect_connect(
        inbound_rx,
        local_keys,
        expected_secret,
        cancel,
        STEP_TIMEOUT,
    )?;

    // Step 2 — reply ack to the signer's connect.
    let ack_response = serde_json::json!({
        "id": connect_id,
        "result": "ack",
    })
    .to_string();
    let signer_pk = nostr::PublicKey::from_hex(&signer_pubkey)
        .map_err(|e| HandshakeError::Protocol(format!("invalid signer pubkey: {e}")))?;
    let ack_ciphertext = nip44::encrypt(
        local_keys.secret_key(),
        &signer_pk,
        ack_response.as_bytes(),
        nip44::Version::V2,
    )
    .map_err(|e| HandshakeError::Protocol(format!("nip44 encrypt ack: {e}")))?;
    let ack_event = EventBuilder::new(Kind::from_u16(24133), ack_ciphertext)
        .tags(vec![
            Tag::parse(["p", &signer_pubkey])
                .map_err(|e| HandshakeError::Protocol(format!("tag parse: {e}")))?,
        ])
        .custom_created_at(Timestamp::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        ))
        .sign_with_keys(local_keys)
        .map_err(|e| HandshakeError::Protocol(format!("sign ack event: {e}")))?;
    let ack_serialized = serde_json::to_string(&ack_event)
        .map_err(|e| HandshakeError::Protocol(format!("serialize ack: {e}")))?;
    relay
        .send(format!(r#"["EVENT",{ack_serialized}]"#))
        .map_err(|e| HandshakeError::Transport(e.to_string()))?;

    // Step 3 — send get_public_key to the signer.
    progress("awaiting_pubkey", Some("Awaiting user confirmation in signer app"));
    let gpk_id = new_request_id();
    publish_rpc(
        relay,
        local_keys,
        signer_pk,
        &gpk_id,
        "get_public_key",
        serde_json::Value::Array(Vec::new()),
    )?;

    // Step 4 — await the get_public_key response.
    let gpk_resp = await_response(
        inbound_rx,
        &gpk_id,
        local_keys,
        signer_pk,
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

    Ok(NostrConnectOutcome {
        signer_pubkey_hex: signer_pubkey,
        user_pubkey_hex: user_pubkey_hex.to_ascii_lowercase(),
    })
}

/// Wait for the first valid `connect` frame from the signer. Returns
/// `(signer_pubkey_hex, connect_request_id)` on success.
///
/// Ignores stray events (not `method:"connect"`, wrong secret, decrypt
/// failures). This allows for old events replayed by the relay despite the
/// `since` filter, without aborting the handshake prematurely.
fn await_nostrconnect_connect(
    inbound_rx: &Receiver<Value>,
    local_keys: &Keys,
    expected_secret: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<(String, String), HandshakeError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(HandshakeError::Cancelled);
        }
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or_else(|| {
                HandshakeError::Timeout(
                    "no connect frame from signer within timeout".to_string(),
                )
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

        // Extract signer pubkey from event.pubkey.
        let signer_pubkey_hex = match event.get("pubkey").and_then(|v| v.as_str()) {
            Some(pk) => pk.to_ascii_lowercase(),
            None => continue, // malformed — skip.
        };
        // Validate: must be 64 hex chars.
        if signer_pubkey_hex.len() != 64
            || !signer_pubkey_hex.chars().all(|c| c.is_ascii_hexdigit())
        {
            continue;
        }
        let Ok(signer_pk) = nostr::PublicKey::from_hex(&signer_pubkey_hex) else { continue };

        let Some(ciphertext) = event.get("content").and_then(|v| v.as_str()) else {
            continue;
        };

        // Decrypt with local_keys.secret + signer_pk.
        let Ok(plaintext) = nip44::decrypt(local_keys.secret_key(), &signer_pk, ciphertext.as_bytes())
        else {
            continue; // not for us or malformed — skip.
        };

        let rpc: serde_json::Value = match serde_json::from_str(&plaintext) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let method = rpc.get("method").and_then(|v| v.as_str()).unwrap_or("");
        if method != "connect" {
            continue; // not a connect frame — skip.
        }

        let id = match rpc.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        let Some(params) = rpc.get("params").and_then(|v| v.as_array()) else { continue };

        // params[1] must match expected_secret.
        let received_secret = params
            .get(1)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if received_secret != expected_secret {
            // Wrong secret — reject with a definitive error (D-NO-HACK).
            return Err(HandshakeError::BunkerError(format!(
                "secret mismatch: expected {expected_secret:?}, got {received_secret:?}"
            )));
        }

        return Ok((signer_pubkey_hex, id));
    }
}

/// Steady-state inbound dispatcher used by `BrokerTransport`: parse a raw
/// kind:24133 event, decrypt the content with NIP-44, and return
/// `(id, result_or_error_json)` for the signer's `deliver_rpc_response`.
/// Returns `None` if the event is malformed or addressed to a different key.
#[must_use]
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

    #[test]
    fn happy_path_connect_then_get_public_key_returns_user_pubkey() {
        let client_keys = Keys::generate();
        let bunker_keys = Keys::generate();
        let bunker_pubkey = bunker_keys.public_key();
        let user_keys = Keys::generate();
        let user_pk_hex = user_keys.public_key().to_hex();

        let (relay, frame_rx) = StubRelay::new();
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();

        let cancel = Arc::new(AtomicBool::new(false));

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
        let (_inbound_tx, inbound_rx) = mpsc::channel::<Value>();

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel);
        // Deterministic trigger: block until the handshake publishes its
        // first outgoing frame (the `connect` RPC), then request cancel.
        // `await_response` re-checks the cancel flag at least every 200ms,
        // so it observes this without any inbound traffic. No sleep needed.
        let canceller = std::thread::spawn(move || {
            let _ = frame_rx.recv();
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
        let _ = canceller.join();
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

    // ─── await_response error / robustness paths ─────────────────────────

    /// The security-critical path: when the bunker replies with an `error`
    /// field, the handshake must surface a `BunkerError` carrying the text —
    /// never silently treat it as success.
    #[test]
    fn run_handshake_surfaces_bunker_error_response() {
        let client_keys = Keys::generate();
        let bunker_keys = Keys::generate();
        let bunker_pubkey = bunker_keys.public_key();

        let (relay, frame_rx) = StubRelay::new();
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();

        let cancel = Arc::new(AtomicBool::new(false));

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
            &client_keys,
            bunker_pubkey,
            None,
            None,
            &cancel,
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
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();

        let cancel = Arc::new(AtomicBool::new(false));

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
            &client_keys,
            bunker_pubkey,
            None,
            None,
            &cancel,
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
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();

        let cancel = Arc::new(AtomicBool::new(false));

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
            &client_keys,
            bunker_pubkey,
            None,
            None,
            &cancel,
            &mut |_, _| {},
        )
        .expect("handshake completes despite stray events");
        assert_eq!(outcome.user_pubkey_hex, user_pk_hex);

        // Dropping the relay closes `frame_tx`; the driver's `recv()`
        // disconnects and the loop exits.
        drop(relay);
        let _ = driver.join();
    }

    // ─── nostrconnect handshake ──────────────────────────────────────────

    /// Helper: build the signer's `connect` event for the nostrconnect flow.
    fn signer_connect_event(
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

    /// Security-critical: a `connect` frame whose `params[1]` secret does not
    /// match the expected session secret must be rejected with a definitive
    /// `BunkerError`, never accepted.
    #[test]
    fn run_nostrconnect_handshake_rejects_secret_mismatch() {
        let client_keys = Keys::generate();
        let signer_keys = Keys::generate();

        let (relay, _drop) = StubRelay::new();
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();

        // Signer sends a connect frame with the WRONG secret.
        let bad = signer_connect_event(&signer_keys, client_keys.public_key(), "wrong-secret");
        inbound_tx.send(bad).unwrap();

        let cancel = Arc::new(AtomicBool::new(false));
        let err = run_nostrconnect_handshake(
            relay.as_ref(),
            &inbound_rx,
            &client_keys,
            "the-real-secret",
            &cancel,
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
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();

        // Deliver the connect frame up front.
        let connect =
            signer_connect_event(&signer_keys, client_keys.public_key(), secret);
        inbound_tx.send(connect).unwrap();

        let cancel = Arc::new(AtomicBool::new(false));

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
            &client_keys,
            secret,
            &cancel,
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
}
