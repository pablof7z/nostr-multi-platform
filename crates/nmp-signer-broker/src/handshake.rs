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
#[path = "handshake/tests.rs"]
mod tests;
