//! Delivery logic for settled sign round-trips in the browser runtime (#2049).
//!
//! Mirrors the `deliver_signer_response` implementation in
//! `nmp-wasm/src/runtime/signer.rs` but adapted for `BrowserRuntime`'s owned
//! (not `RefCell`) reducer and the `mpsc` completion channel model.
//!
//! # D4 single-writer
//!
//! `deliver_one_completion` is called exclusively from inside `BrowserRuntime::pump()`
//! (via `&mut BrowserRuntime`) — the single write point for the `KernelReducer`.
//! The async NIP-07 driver in `signer/completion.rs` uses only the channel
//! sender; it never touches the reducer.
//!
//! # D8 no polling
//!
//! `deliver_signed_response_at` and `fail_sign_roundtrip_at` are one-shot
//! message re-entry calls — no poll loop, no blocking recv.

use std::collections::HashMap;

use nmp_core::time::Instant;
use nmp_core::{KernelReducer, OutboundMessage, SignRoundTripOutcome};

use super::event::BrowserRuntimeEvent;
use super::PendingSignedPublish;
use crate::signer::SignerCompletion;

/// Parse a flat-NIP-01 signed event JSON string into a [`nmp_store::RawEvent`].
///
/// Total (D6): returns `Err(reason)` on any shape mismatch — never panics.
fn signed_json_to_raw_event(signed_json: &str) -> Result<nmp_store::RawEvent, String> {
    serde_json::from_str(signed_json)
        .map_err(|e| format!("signed event JSON did not decode as RawEvent: {e}"))
}

/// Deliver one settled sign completion to the kernel and return any resulting
/// outbound frames and host events.
///
/// Called by `BrowserRuntime::pump()` for each item drained from the
/// completion channel, AND by `BrowserRuntimeHandle::deliver_signer_response`
/// for host-brokered deliveries.
pub(super) fn deliver_one_completion(
    reducer: &mut KernelReducer,
    pending: &mut HashMap<String, PendingSignedPublish>,
    completion: SignerCompletion,
) -> (Vec<OutboundMessage>, Vec<BrowserRuntimeEvent>) {
    let now = Instant::now();
    let outcome = match completion.result {
        Ok(signed_json) => {
            reducer.deliver_signed_response_at(&completion.correlation_id, &signed_json, now)
        }
        Err(reason) => reducer.fail_sign_roundtrip_at(&completion.correlation_id, &reason, now),
    };
    settle_outcome(reducer, pending, outcome)
}

/// Map a `SignRoundTripOutcome` to outbound frames + host events.
///
/// On `Completed`: pops the parked publish, parses the signed JSON into a
/// `RawEvent`, calls `reducer.publish_pre_signed`, and emits `SignCompleted`
/// so the main-thread broker can resolve its pending promise (#2139 BLOCKER 2).
/// On `Failed`: emits `SignFailed` (not `CommandFailed`) — the wire protocol
/// expects a correlation-keyed sign terminal the main-thread broker can resolve.
/// On `Unknown` (stale/duplicate delivery): surfaces a `SignFailed` event
/// (D6 — never a silent drop; mirrors `nmp-wasm`'s `WorkerEvent::SignFailed`).
fn settle_outcome(
    reducer: &mut KernelReducer,
    pending: &mut HashMap<String, PendingSignedPublish>,
    outcome: SignRoundTripOutcome,
) -> (Vec<OutboundMessage>, Vec<BrowserRuntimeEvent>) {
    match outcome {
        SignRoundTripOutcome::Completed {
            correlation_id,
            signed_json,
        } => {
            let parked = pending.remove(&correlation_id);
            match (parked, signed_json_to_raw_event(&signed_json)) {
                (Some(p), Ok(raw)) => {
                    let outbound =
                        reducer.publish_pre_signed(raw, p.target, p.action_correlation_id);
                    // Emit SignCompleted so the main-thread broker knows the
                    // round-trip settled (#2139 BLOCKER 2 — was Vec::new()).
                    let events = vec![BrowserRuntimeEvent::SignCompleted {
                        correlation_id,
                        signed_json,
                    }];
                    (outbound, events)
                }
                (Some(_), Err(reason)) => {
                    // Signed JSON came back but doesn't decode as RawEvent — fail closed.
                    let events = vec![BrowserRuntimeEvent::CommandFailed { reason }];
                    (Vec::new(), events)
                }
                (None, _) => {
                    // No parked entry: bare sign-only round-trip or already resolved.
                    // Still emit SignCompleted so the main thread can settle (#2139).
                    let events = vec![BrowserRuntimeEvent::SignCompleted {
                        correlation_id,
                        signed_json,
                    }];
                    (Vec::new(), events)
                }
            }
        }
        SignRoundTripOutcome::Failed {
            correlation_id,
            reason,
        } => {
            pending.remove(&correlation_id);
            // Emit SignFailed (not CommandFailed) — the wire protocol expects a
            // correlation-keyed sign terminal the main-thread broker can resolve
            // (#2139 BLOCKER 2 — was CommandFailed which breaks broker settlement).
            let events = vec![BrowserRuntimeEvent::SignFailed {
                correlation_id,
                reason,
            }];
            (Vec::new(), events)
        }
        SignRoundTripOutcome::Unknown { correlation_id } => {
            // Stale or duplicate delivery: the kernel had no parked round-trip
            // for this id. Surface it honestly (D6) rather than dropping it —
            // a stranded sign delivery must be observable to the host.
            let events = vec![BrowserRuntimeEvent::SignFailed {
                correlation_id,
                reason: "no parked sign round-trip matched this correlation id \
                         (stale or duplicate delivery)"
                    .to_string(),
            }];
            (Vec::new(), events)
        }
    }
}
