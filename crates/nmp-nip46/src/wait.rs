//! Event-driven blocking waits for the NIP-46 handshake state machine.
//!
//! Both [`await_response`] and [`recv_inbound_or_cancel`] are the
//! shared wait primitives consumed by [`crate::bunker::run_handshake`] and
//! [`crate::nostrconnect::run_nostrconnect_handshake`].
//!
//! ## D8 — no polling
//!
//! Every wait `select!`s over three receivers and blocks until exactly one is
//! ready. There is no timer-driven re-check loop:
//!
//! - the **inbound** channel (`Receiver<Value>`) — a relay event arrived;
//! - a one-shot **cancel** channel (`Receiver<()>`) — the broker cancelled the
//!   session (a delivered `()` or a `Disconnected` both mean "cancelled");
//! - a single **deadline** timer (`crossbeam_channel::after`) — the per-step
//!   wall-clock budget elapsed.
//!
//! This is a STEP-1 carry: STEP 2 replaces the blocking wait with a reducer
//! that the broker's event loop drives, at which point this module goes away.

use std::time::Duration;

use crossbeam_channel::Receiver;
use nostr::nips::nip44;
use nostr::{Keys, PublicKey};
use serde_json::Value;

use crate::error::HandshakeError;

/// Block waiting for the response to `expected_id`.
///
/// The receiver carries the raw event JSON (the third element of
/// `["EVENT", sub_id, event_json]`). Each event is decrypted with NIP-44,
/// parsed as JSON-RPC, and matched on `id`. Stray events (wrong pubkey,
/// undecryptable, wrong id) are skipped silently (D6).
///
/// The wait is event-driven (D8): each iteration blocks in `select!` over the
/// inbound channel, the cancel channel, and a single deadline timer set to the
/// *remaining* budget. The loop only re-enters when a matching event has not
/// yet arrived; the deadline timer is re-armed to the shrinking remaining
/// budget so the overall step bound holds.
pub(crate) fn await_response(
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
        let event =
            recv_inbound_or_cancel(inbound_rx, cancel_rx, deadline, method_label, timeout)?;
        let Some(ciphertext) = event.get("content").and_then(|v| v.as_str()) else {
            continue;
        };
        let event_pubkey = event.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
        if event_pubkey.to_ascii_lowercase() != remote_pubkey.to_hex() {
            // Stray event addressed to us from a different signer; ignore.
            continue;
        }
        // D6: a decrypt failure means this event is not for us — skip silently.
        let Ok(plaintext) =
            nip44::decrypt(local_keys.secret_key(), &remote_pubkey, ciphertext.as_bytes())
        else {
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
pub(crate) fn recv_inbound_or_cancel(
    inbound_rx: &Receiver<Value>,
    cancel_rx: &Receiver<()>,
    deadline: std::time::Instant,
    method_label: &str,
    timeout: Duration,
) -> Result<Value, HandshakeError> {
    let remaining = deadline
        .checked_duration_since(std::time::Instant::now())
        .ok_or_else(|| {
            HandshakeError::Timeout(format!(
                "no response to {method_label} within {timeout:?}"
            ))
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
