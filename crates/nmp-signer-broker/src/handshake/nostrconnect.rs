//! `nostrconnect://` (signer-initiated) handshake.
//!
//! The signer scans the client's QR code and initiates the connection. This is
//! the mirror of the client-initiated `bunker://` flow in the parent module:
//! it reuses the same event-driven waits ([`super::await_response`],
//! [`super::recv_inbound_or_cancel`]) and RPC helpers ([`super::publish_rpc`],
//! [`super::new_request_id`]) — only the first step differs (wait for the
//! signer's `connect` frame instead of sending one).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::Receiver;
use nostr::nips::nip44;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::Value;

use super::{
    await_response, new_request_id, publish_rpc, recv_inbound_or_cancel, HandshakeError,
    STEP_TIMEOUT,
};
use crate::relay_client::RelayClient;

/// Result of a successful nostrconnect:// handshake: the signer's pubkey and
/// the user's pubkey (as returned by `get_public_key`).
#[derive(Debug, Clone)]
pub struct NostrConnectOutcome {
    /// The remote signer's pubkey (learned from `event.pubkey` of the first
    /// inbound `connect` frame). Needed to construct the `BrokerTransport`.
    pub signer_pubkey_hex: String,
    /// The user pubkey returned by `get_public_key` — what
    /// the completed signer reports to the host adapter.
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
pub fn run_nostrconnect_handshake(
    relay: &dyn RelayClient,
    inbound_rx: &Receiver<Value>,
    cancel_rx: &Receiver<()>,
    local_keys: &Keys,
    expected_secret: &str,
    progress: &mut dyn FnMut(&str, &str, Option<&str>),
) -> Result<NostrConnectOutcome, HandshakeError> {
    // Step 1 — wait for the signer's connect event.
    progress(
        "connecting",
        crate::progress_codes::NOSTRCONNECT_SCAN_QR,
        Some("Waiting for signer to scan QR code"),
    );
    let (signer_pubkey, connect_id) = await_nostrconnect_connect(
        inbound_rx,
        cancel_rx,
        local_keys,
        expected_secret,
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
        .tags(vec![Tag::parse(["p", &signer_pubkey]).map_err(|e| {
            HandshakeError::Protocol(format!("tag parse: {e}"))
        })?])
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
    progress(
        "awaiting_pubkey",
        crate::progress_codes::NOSTRCONNECT_AWAITING_CONFIRMATION,
        Some("Awaiting user confirmation in signer app"),
    );
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
        cancel_rx,
        &gpk_id,
        local_keys,
        signer_pk,
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
    cancel_rx: &Receiver<()>,
    local_keys: &Keys,
    expected_secret: &str,
    timeout: Duration,
) -> Result<(String, String), HandshakeError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        // Event-driven wait (D8 — no polling): blocks until a frame arrives,
        // cancellation, or the step deadline. No timer-driven re-check.
        let event = recv_inbound_or_cancel(
            inbound_rx,
            cancel_rx,
            deadline,
            "connect frame from signer",
            timeout,
        )?;

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
        let Ok(signer_pk) = nostr::PublicKey::from_hex(&signer_pubkey_hex) else {
            continue;
        };

        let Some(ciphertext) = event.get("content").and_then(|v| v.as_str()) else {
            continue;
        };

        // Decrypt with local_keys.secret + signer_pk.
        let Ok(plaintext) =
            nip44::decrypt(local_keys.secret_key(), &signer_pk, ciphertext.as_bytes())
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

        let Some(params) = rpc.get("params").and_then(|v| v.as_array()) else {
            continue;
        };

        // params[1] must match expected_secret.
        let received_secret = params.get(1).and_then(|v| v.as_str()).unwrap_or("");
        if received_secret != expected_secret {
            // Wrong secret — reject with a definitive error (D-NO-HACK).
            return Err(HandshakeError::BunkerError(format!(
                "secret mismatch: expected {expected_secret:?}, got {received_secret:?}"
            )));
        }

        return Ok((signer_pubkey_hex, id));
    }
}
