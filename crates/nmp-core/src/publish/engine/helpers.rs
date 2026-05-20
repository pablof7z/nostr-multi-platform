//! Engine-internal helpers (no public surface). Separated so the orchestrator
//! file stays under the file-size soft cap.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::super::action::{PublishOutcome, RelayUrl};
use super::super::state::{PerRelayState, RelayAck, RetryVerdict};
use super::super::traits::RelayDispatcher;
use super::super::view::{PublishStatusState, RecentFailure, RecentSuccess};
use super::{InFlight, TerminalOutcome};
use crate::substrate::SignedEvent;

pub(super) fn relay_url_of(ack: &RelayAck) -> RelayUrl {
    ack.relay_url.clone()
}

pub(super) fn dispatch_due(
    in_flight: &mut InFlight,
    now_ms: u64,
    dispatcher: &dyn RelayDispatcher,
    frame: &str,
    relay_filter: Option<&str>,
    unavailable_relays: &BTreeSet<RelayUrl>,
) -> Vec<RelayAck> {
    let mut acks = Vec::new();
    for (relay_url, state) in in_flight.per_relay.iter_mut() {
        if let Some(filter) = relay_filter {
            if relay_url != filter {
                continue;
            }
        }
        if unavailable_relays.contains(relay_url) {
            continue;
        }
        let ready = match state {
            PerRelayState::Pending => true,
            PerRelayState::RelayError { .. } | PerRelayState::TimedOut { .. } => {
                // No pending_retries entry → restart-resumed state: retry now.
                // With an entry → retry once now_ms catches up.
                in_flight
                    .pending_retries
                    .get(relay_url)
                    .map(|due| *due <= now_ms)
                    .unwrap_or(true)
            }
            _ => false,
        };
        if !ready {
            continue;
        }
        let attempt = state.attempt().saturating_add(1).max(1);
        *state = PerRelayState::InFlight {
            sent_at_ms: now_ms,
            attempt,
        };
        in_flight.pending_retries.remove(relay_url);
        in_flight.dirty = true;
        acks.extend(dispatcher.dispatch(relay_url, frame));
    }
    acks
}

pub(super) fn apply_verdict(
    in_flight: &mut InFlight,
    relay_url: &str,
    verdict: RetryVerdict,
    now_ms: u64,
) {
    let Some(state) = in_flight.per_relay.get_mut(relay_url) else {
        return;
    };
    match verdict {
        RetryVerdict::Settled(next) => {
            *state = next;
            in_flight.dirty = true;
        }
        RetryVerdict::ScheduleRetry {
            delay_ms,
            next_attempt,
        } => {
            *state = PerRelayState::RelayError {
                message: format!("retry scheduled (attempt {})", next_attempt),
                attempt: next_attempt - 1,
                last_at_ms: now_ms,
            };
            in_flight
                .pending_retries
                .insert(relay_url.to_string(), now_ms.saturating_add(delay_ms));
            in_flight.dirty = true;
        }
        RetryVerdict::Reauth {
            delay_ms,
            next_attempt,
        } => {
            // M6 signer integration: the engine will call sign_auth, dispatch
            // AUTH, then re-dispatch the original publish on success. Until M6
            // lands the auth-required path is modelled as a transient retry —
            // the test in tests.rs exercises this by re-feeding the original
            // event on the next dispatch tick.
            *state = PerRelayState::RelayError {
                message: format!("auth-required, reauth attempt {}", next_attempt),
                attempt: next_attempt - 1,
                last_at_ms: now_ms,
            };
            in_flight
                .pending_retries
                .insert(relay_url.to_string(), now_ms.saturating_add(delay_ms));
            in_flight.dirty = true;
        }
    }
}

pub(super) fn is_complete(in_flight: &InFlight) -> bool {
    in_flight
        .per_relay
        .values()
        .all(|state| state.is_terminal())
}

pub(super) fn for_each_terminal(
    in_flight: &InFlight,
    handle: &str,
    view: &mut PublishStatusState,
    now_ms: u64,
) {
    let mut accepted: Vec<RelayUrl> = Vec::new();
    let mut failures: Vec<(RelayUrl, String)> = Vec::new();
    for (relay_url, state) in &in_flight.per_relay {
        match state {
            PerRelayState::Ok { .. } => accepted.push(relay_url.clone()),
            PerRelayState::FailedAfterRetries { reason, .. } => {
                failures.push((relay_url.clone(), reason.clone()));
            }
            _ => {}
        }
    }
    if !accepted.is_empty() {
        view.push_success(RecentSuccess {
            handle: handle.to_string(),
            event_id: in_flight.event.id.clone(),
            accepted_by: accepted,
            at_ms: now_ms,
        });
    }
    for (relay_url, reason) in failures {
        view.push_failure(RecentFailure {
            handle: handle.to_string(),
            event_id: in_flight.event.id.clone(),
            relay_url,
            reason,
            at_ms: now_ms,
        });
    }
}

/// T128: snapshot the per-relay terminal verdict for a fully-settled
/// `InFlight` row. Called from `engine::on_ack` right before the row is
/// evicted from `in_flight`. Mirrors `for_each_terminal` but in a shape the
/// kernel consumes directly (no `RecentSuccess` / `RecentFailure` indirection
/// — those are for the engine's bounded ring buffers; the kernel's queue
/// entry needs the full per-relay map).
pub(super) fn terminal_outcome_of(in_flight: &InFlight) -> TerminalOutcome {
    let mut accepted: Vec<RelayUrl> = Vec::new();
    let mut failed: Vec<(RelayUrl, String)> = Vec::new();
    for (relay_url, state) in &in_flight.per_relay {
        match state {
            PerRelayState::Ok { .. } => accepted.push(relay_url.clone()),
            PerRelayState::FailedAfterRetries { reason, .. } => {
                failed.push((relay_url.clone(), reason.clone()));
            }
            _ => {}
        }
    }
    TerminalOutcome {
        event_id: in_flight.event.id.clone(),
        accepted,
        failed,
    }
}

pub(super) fn build_event_frame(event: &SignedEvent) -> String {
    let body = json!({
        "id": event.id,
        "pubkey": event.unsigned.pubkey,
        "created_at": event.unsigned.created_at,
        "kind": event.unsigned.kind,
        "tags": event.unsigned.tags,
        "content": event.unsigned.content,
        "sig": event.sig,
    });
    json!(["EVENT", body]).to_string()
}

pub(super) fn collect_p_tags(event: &SignedEvent) -> Vec<String> {
    let mut out = BTreeSet::new();
    for tag in &event.unsigned.tags {
        if tag.len() >= 2 && tag[0] == "p" {
            out.insert(tag[1].clone());
        }
    }
    out.into_iter().collect()
}

/// Coarse outcome computed from the current per-relay states. Used by the
/// ledger to record a single verdict for the publish.
pub fn outcome_of(per_relay: &BTreeMap<RelayUrl, PerRelayState>) -> PublishOutcome {
    let mut accepted = Vec::new();
    let mut failed = Vec::new();
    for (relay_url, state) in per_relay {
        match state {
            PerRelayState::Ok { .. } => accepted.push(relay_url.clone()),
            PerRelayState::FailedAfterRetries { .. } => failed.push(relay_url.clone()),
            _ => {}
        }
    }
    match (accepted.is_empty(), failed.is_empty()) {
        (false, true) => PublishOutcome::Accepted { relays: accepted },
        (false, false) => PublishOutcome::Mixed { accepted, failed },
        (true, false) => PublishOutcome::FailedAfterRetries { failed },
        (true, true) => PublishOutcome::NoTargets,
    }
}
