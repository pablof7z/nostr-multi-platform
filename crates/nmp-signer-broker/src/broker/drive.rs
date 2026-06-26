//! The `drive` helper: applies start effects then runs the event-select loop.
//!
//! This is what `wait.rs` in nmp-nip46 used to be — the blocking
//! `crossbeam::select_biased!` loop that routes inbound relay events into
//! the pure-function reducer, checks the cancel signal, and fires the
//! step-deadline timer. Moving it here means the reducer itself (nmp-nip46)
//! has NO blocking, NO crossbeam, NO threads — making it wasm-safe.
//!
//! The nmp-nip46 reducer is pure (no I/O, no time), and this module is the
//! thin I/O shell that drives it on a native thread. wasm callers drive the
//! reducer directly with their own event loop.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::Receiver as CbReceiver;
use nmp_nip46::{Effect, HandshakeError, SessionState, SignerReady};
use serde_json::Value;

use crate::relay_client::RelayClient;

/// Terminal result of a driven handshake session.
pub enum DriveOutcome {
    /// The signer confirmed its identity; carry the session result forward.
    Ready(SignerReady),
    /// The handshake failed (timeout, protocol error, bunker error, etc.).
    Failed(HandshakeError),
    /// The session was cancelled before completing.
    Cancelled,
}

/// Drive `reducer` to completion using the already-connected `relay`.
///
/// 1. Applies `start_effects` in order (Subscribe → relay.subscribe,
///    SendFrame → relay.send, Progress → emit_progress, Error → return).
/// 2. Enters a `select_biased!` loop (`cancel > deadline > inbound`) feeding
///    relay events into the reducer and dispatching returned effects until the
///    reducer reaches a terminal state (SignerReady or Error).
///
/// `relay` must already be connected. `cancel_rx` is the one-shot channel
/// that [`crate::broker::BunkerBroker::cancel`] signals. `emit_progress` is
/// `(stage, code, detail)` — the broker passes `emit_progress_coded` for all
/// handshake stages (codes are always present on effects the reducer emits).
pub fn drive(
    reducer: &mut SessionState,
    relay: &dyn RelayClient,
    start_effects: Vec<Effect>,
    inbound_rx: &CbReceiver<Value>,
    cancel_rx: &CbReceiver<()>,
    emit_progress: &mut dyn FnMut(&str, &str, Option<&str>),
) -> DriveOutcome {
    // Phase 1: apply start effects (subscribe, initial send, initial progress).
    if let Some(outcome) = apply_effects(start_effects, relay, emit_progress) {
        return outcome;
    }

    // Phase 2: event loop — cancel > deadline > inbound (D8 — no polling).
    loop {
        let now = now_secs();
        let deadline = reducer.deadline_at();
        let remaining = deadline.saturating_sub(now);
        let timeout = crossbeam_channel::after(Duration::from_secs(remaining));

        crossbeam_channel::select_biased! {
            recv(cancel_rx) -> _ => return DriveOutcome::Cancelled,
            recv(timeout) -> _ => {
                let effects = reducer.tick(now_secs());
                if let Some(outcome) = apply_effects(effects, relay, emit_progress) {
                    return outcome;
                }
            },
            recv(inbound_rx) -> msg => {
                match msg {
                    Ok(event) => {
                        let effects = reducer.on_relay_event(&event, now_secs());
                        if let Some(outcome) = apply_effects(effects, relay, emit_progress) {
                            return outcome;
                        }
                    }
                    Err(_) => {
                        // Channel disconnected — session cancelled or dropped.
                        return DriveOutcome::Cancelled;
                    }
                }
            }
        }
    }
}

/// Apply a batch of effects. Returns `Some(DriveOutcome)` for terminal effects
/// (SignerReady, Error) or relay send failures; returns `None` when non-terminal
/// (loop should continue).
fn apply_effects(
    effects: Vec<Effect>,
    relay: &dyn RelayClient,
    emit_progress: &mut dyn FnMut(&str, &str, Option<&str>),
) -> Option<DriveOutcome> {
    for effect in effects {
        match effect {
            Effect::Subscribe { frame, .. } => {
                if let Err(e) = relay.subscribe(frame) {
                    return Some(DriveOutcome::Failed(HandshakeError::Transport(e.to_string())));
                }
            }
            Effect::SendFrame { text, .. } => {
                if let Err(e) = relay.send(text) {
                    return Some(DriveOutcome::Failed(HandshakeError::Transport(e.to_string())));
                }
            }
            Effect::Progress { stage, code, detail } => {
                emit_progress(
                    &stage,
                    code.as_deref().unwrap_or(""),
                    detail.as_deref(),
                );
            }
            Effect::SignerReady(sr) => {
                return Some(DriveOutcome::Ready(sr));
            }
            Effect::Error { error } => {
                return Some(DriveOutcome::Failed(error));
            }
            Effect::DeliverResponse { .. } => {
                // Steady-state RPC responses are not part of the handshake; ignore.
            }
        }
    }
    None
}

/// Current Unix-second timestamp. Used for the `now` argument to reducer
/// entry points on the native thread path. This function MUST NOT appear on
/// any path that runs in wasm (hence it lives here rather than in nmp-nip46).
pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
