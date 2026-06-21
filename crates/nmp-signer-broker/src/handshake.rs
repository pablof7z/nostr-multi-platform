//! NIP-46 handshake state machine.
//!
//! Pure-ish module: takes a `RelayClient` impl and a `Keys` (local ephemeral
//! key), runs the `connect` → `get_public_key` dance, returns the user's
//! pubkey. Side effects are limited to: publishing on the relay client and
//! receiving inbound events via a `Receiver<Value>` it sets up.
//!
//! ## Waiting is event-driven (D8 — no polling)
//!
//! Every wait in this module `select!`s over three receivers and blocks until
//! exactly one is ready — it never wakes on a timer to re-check a flag:
//!
//! - the **inbound** channel (`Receiver<Value>`) — a relay event arrived;
//! - a one-shot **cancel** channel (`Receiver<()>`) — the broker cancelled the
//!   session (`cancel()` sends `()` and drops its sender; either a delivered
//!   `()` or the resulting disconnect wakes the `select!`);
//! - a single **deadline** timer (`crossbeam_channel::after`) — the per-step
//!   wall-clock budget elapsed. This fires once at the deadline; it is a bound,
//!   not a re-check loop.
//!
//! This replaced an earlier `recv_timeout(200ms)` loop that existed only to
//! re-poll a cancel `AtomicBool` between blocking receives.
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

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::Receiver;
use nostr::nips::nip44;
use nostr::{EventBuilder, Keys, Kind, PublicKey, Tag, Timestamp};
use serde_json::{json, Value};

use crate::relay_client::RelayClient;

mod nostrconnect;
pub use nostrconnect::{run_nostrconnect_handshake, NostrConnectOutcome};

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
    /// the completed signer reports to the host adapter.
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
/// `progress` is an `&mut dyn FnMut(&str, &str, Option<&str>)` — `(stage, code,
/// message)` — so the broker can
/// publish progress events to the host adapter. The handshake itself emits two
/// transitions: `"connecting"` (before `connect`) and `"awaiting_pubkey"`
/// (before `get_public_key`). The final `"ready"` is emitted by the broker
/// after constructing the signer.
#[allow(clippy::too_many_arguments)] // protocol state machine — eight closely related inputs
pub fn run_handshake(
    relay: &dyn RelayClient,
    inbound_rx: &Receiver<Value>,
    cancel_rx: &Receiver<()>,
    local_keys: &Keys,
    remote_pubkey: PublicKey,
    secret: Option<&str>,
    perms: Option<&str>,
    progress: &mut dyn FnMut(&str, &str, Option<&str>),
) -> Result<HandshakeOutcome, HandshakeError> {
    // Step 1 — connect.
    progress(
        "connecting",
        crate::progress_codes::SENDING_CONNECT_TO_BUNKER,
        Some("Sending connect to bunker"),
    );
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
        cancel_rx,
        &connect_id,
        local_keys,
        remote_pubkey,
        STEP_TIMEOUT,
        "connect",
    )?;

    // Step 2 — get_public_key.
    progress(
        "awaiting_pubkey",
        crate::progress_codes::AWAITING_BUNKER_APPROVAL,
        Some("Awaiting bunker approval"),
    );
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
        cancel_rx,
        &gpk_id,
        local_keys,
        remote_pubkey,
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
///
/// The wait is event-driven (D8 — no polling): each iteration blocks in a
/// `select!` over the inbound channel, the cancel channel, and a single
/// deadline timer set to the *remaining* budget. There is no timer-driven
/// re-check — the loop only re-enters when a matching event has not yet
/// arrived (a stray/other-RPC event was dropped), and the deadline timer is
/// re-armed to the shrinking remaining budget so the overall step bound holds.
fn await_response(
    inbound_rx: &Receiver<Value>,
    cancel_rx: &Receiver<()>,
    expected_id: &str,
    local_keys: &Keys,
    remote_pubkey: PublicKey,
    timeout: Duration,
    method_label: &str,
) -> Result<String, HandshakeError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let event = recv_inbound_or_cancel(inbound_rx, cancel_rx, deadline, method_label, timeout)?;
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
                let msg = err.as_str().map_or_else(|| err.to_string(), str::to_string);
                return Err(HandshakeError::BunkerError(msg));
            }
        }
        let result = rpc.get("result").and_then(|v| v.as_str()).ok_or_else(|| {
            HandshakeError::Protocol(format!("{method_label} response missing string result"))
        })?;
        return Ok(result.to_string());
    }
}

/// Block until the next inbound event, cancellation, or the step deadline,
/// whichever comes first — the single event-driven wait both handshake loops
/// share (D8 — no polling).
///
/// `select!`s over three receivers and blocks until exactly one is ready:
/// - `inbound_rx` — returns the event;
/// - `cancel_rx` — returns [`HandshakeError::Cancelled`]. A delivered `()`
///   *or* a `Disconnected` (the broker dropped its `cancel_tx`) both mean
///   "cancelled"; either wakes the `select!` immediately;
/// - a `crossbeam_channel::after(remaining)` timer — returns
///   [`HandshakeError::Timeout`]. It fires once at the deadline; re-arming it
///   to the shrinking `remaining` each loop iteration keeps the overall step
///   bound without ever waking early to re-check a flag.
///
/// An inbound `Disconnected` (every sender dropped without a cancel) surfaces
/// as [`HandshakeError::Transport`].
fn recv_inbound_or_cancel(
    inbound_rx: &Receiver<Value>,
    cancel_rx: &Receiver<()>,
    deadline: std::time::Instant,
    method_label: &str,
    timeout: Duration,
) -> Result<Value, HandshakeError> {
    let remaining = deadline
        .checked_duration_since(std::time::Instant::now())
        .ok_or_else(|| {
            HandshakeError::Timeout(format!("no response to {method_label} within {timeout:?}"))
        })?;
    let deadline_rx = crossbeam_channel::after(remaining);
    crossbeam_channel::select_biased! {
        // Cancel wins over queued inbound noise.
        recv(cancel_rx) -> _ => Err(HandshakeError::Cancelled),
        // Deadline before inbound so a flooded stale-event queue cannot
        // starve the step deadline.
        recv(deadline_rx) -> _ => Err(HandshakeError::Timeout(format!(
            "no response to {method_label} within {timeout:?}"
        ))),
        recv(inbound_rx) -> msg => msg.map_err(|_| {
            HandshakeError::Transport("inbound channel disconnected".to_string())
        }),
    }
}

/// Steady-state inbound dispatcher used by `BrokerTransport`: parse a raw
/// kind:24133 event, decrypt the content with NIP-44, and return
/// `(id, result_or_error_json)` for the signer's `deliver_response`.
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
#[path = "handshake/tests/mod.rs"]
mod tests;
