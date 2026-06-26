//! Client-initiated (`bunker://` URI form) handshake state machine.
//!
//! Pure module: takes a [`crate::relay::FrameSink`] impl and a `Keys` (local
//! ephemeral key), runs the `connect` → `get_public_key` dance, returns the
//! user's pubkey. Side effects are limited to: publishing via the frame sink
//! and receiving inbound events on the supplied `Receiver<Value>`.
//!
//! ## Protocol shape
//!
//! 1. **Subscribe** — handled by the broker BEFORE calling `run_handshake`
//!    (the REQ frame is sent via `relay_client.subscribe()` to survive
//!    reconnects; this crate has no socket knowledge).
//! 2. **connect**: send `{method:"connect", params:[remote, secret, perms]}`
//!    NIP-44-encrypted to `remote_pubkey`, wrapped in kind:24133 tagged
//!    `["p", remote]`. Accept any non-error response.
//! 3. **get_public_key**: same envelope. Response `result` is the user's
//!    pubkey hex.
//!
//! ## D8 — no polling
//!
//! All waits are event-driven via [`crate::wait`]. No re-check loops.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::Receiver;
use nostr::{Keys, PublicKey};
use serde_json::{json, Value};

use crate::error::HandshakeError;
use crate::relay::FrameSink;
use crate::rpc::{build_connect_params, build_event_frame, new_request_id};
use crate::wait::await_response;

/// Result of a successful handshake: the user's pubkey hex.
#[derive(Debug, Clone)]
pub struct HandshakeOutcome {
    /// The user's pubkey, returned by `get_public_key`. This is what the
    /// completed signer reports to the host adapter.
    pub user_pubkey_hex: String,
}

/// Per-handshake step deadline. The bunker often needs the user to tap approve
/// on the phone; ~60 s covers normal UX.
pub(crate) const STEP_TIMEOUT: Duration = Duration::from_secs(60);

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

/// Run the client-initiated (`bunker://`) handshake.
///
/// Caller owns the relay frame sink (already connected + subscribed) and the
/// inbound event receiver. Returns the user pubkey on success.
///
/// `progress` is an `&mut dyn FnMut(&str, &str, Option<&str>)` —
/// `(stage, code, message)` — so the broker can publish progress events to the
/// host adapter. The handshake itself emits two transitions: `"connecting"`
/// (before `connect`) and `"awaiting_pubkey"` (before `get_public_key`). The
/// final `"ready"` is emitted by the broker after constructing the signer.
#[allow(clippy::too_many_arguments)] // protocol state machine — eight closely related inputs
pub fn run_handshake(
    relay: &dyn FrameSink,
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
    let connect_envelope = json!({
        "id": &connect_id,
        "method": "connect",
        "params": connect_params,
    })
    .to_string();
    let frame = build_event_frame(local_keys, remote_pubkey, &connect_envelope)
        .map_err(|e| HandshakeError::Protocol(e.to_string()))?;
    relay
        .send(frame)
        .map_err(|e| HandshakeError::Transport(e.to_string()))?;

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
    let gpk_envelope = json!({
        "id": &gpk_id,
        "method": "get_public_key",
        "params": Value::Array(Vec::new()),
    })
    .to_string();
    let frame = build_event_frame(local_keys, remote_pubkey, &gpk_envelope)
        .map_err(|e| HandshakeError::Protocol(e.to_string()))?;
    relay
        .send(frame)
        .map_err(|e| HandshakeError::Transport(e.to_string()))?;

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
