//! Engine-internal helpers (no public surface). Separated so the orchestrator
//! file stays under the file-size soft cap.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::super::action::{PublishOutcome, RelayUrl};
use super::super::state::{PerRelayState, RelayAck, RetryPolicy, RetryVerdict};
use super::super::traits::RelayDispatcher;
use super::super::view::{PublishStatusState, RecentFailure, RecentSuccess};
use super::{InFlight, TerminalOutcome};
use crate::relay::CanonicalRelayUrl;
use crate::substrate::SignedEvent;

pub(super) fn canonical_relay_identity(raw: &str) -> RelayUrl {
    CanonicalRelayUrl::parse_or_raw(raw).into_string()
}

pub(super) fn relay_url_of(ack: &RelayAck) -> RelayUrl {
    canonical_relay_identity(&ack.relay_url)
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
    for (relay_url, state) in &mut in_flight.per_relay {
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
                    .is_none_or(|due| *due <= now_ms)
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

/// Fold a verdict into the per-relay state. Returns `true` when the verdict is
/// `ParkAwaitingAuth` — the caller (`engine::on_ack`) must then route the relay
/// through the availability gate (`mark_relay_unavailable`), which owns
/// `unavailable_relays` and performs the InFlight→Pending demotion. The park
/// is deliberately NOT handled here: this free fn only sees `&mut InFlight` and
/// has no access to the engine's `unavailable_relays` set, and the single
/// availability-gate mechanism must own every "this relay can't take a publish
/// right now" transition (D4: one writer per fact, no parallel auth-park path).
pub(super) fn apply_verdict(
    in_flight: &mut InFlight,
    relay_url: &str,
    verdict: RetryVerdict,
    now_ms: u64,
) -> bool {
    let Some(state) = in_flight.per_relay.get_mut(relay_url) else {
        return false;
    };
    match verdict {
        RetryVerdict::Settled(next) => {
            *state = next;
            in_flight.dirty = true;
            false
        }
        RetryVerdict::ScheduleRetry {
            delay_ms,
            next_attempt,
        } => {
            *state = PerRelayState::RelayError {
                message: format!("retry scheduled (attempt {next_attempt})"),
                attempt: next_attempt - 1,
                last_at_ms: now_ms,
            };
            in_flight
                .pending_retries
                .insert(relay_url.to_string(), now_ms.saturating_add(delay_ms));
            in_flight.dirty = true;
            false
        }
        // The relay refused the EVENT pending NIP-42 auth. Leave the state for
        // the availability gate to demote (InFlight→Pending) and signal the
        // park up to the caller; never schedule a retry (D8: the re-dispatch is
        // event-driven off `mark_relay_available`, fired when the socket
        // reaches `Authenticated`).
        RetryVerdict::ParkAwaitingAuth { .. } => true,
    }
}

pub(super) fn is_complete(in_flight: &InFlight) -> bool {
    in_flight
        .per_relay
        .values()
        .all(super::super::state::PerRelayState::is_terminal)
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
    // Clone the captured-at-publish-time rationale map verbatim so the kernel
    // projection can render the same "why was this relay targeted?" string the
    // in-flight outbox shows. Keys mirror `per_relay` (the engine seeded both
    // from the same `relay_map` in `start_publish_inner`).
    TerminalOutcome {
        event_id: in_flight.event.id.clone(),
        accepted,
        failed,
        relay_reasons: in_flight.relay_reasons.clone(),
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

/// Transition any `InFlight` relay whose send predates `now_ms - deadline_ms`.
/// If the attempt count has exhausted the transient retry budget, transitions
/// directly to `FailedAfterRetries`; otherwise to `TimedOut` so the existing
/// retry ladder can pick it up. Returns `true` if any row changed.
///
/// A relay that accepts the TCP connection but never sends `OK` (and never
/// closes the socket) would otherwise pin a publish in `InFlight` forever.
/// This sweeper runs in `PublishEngine::tick` so a stuck publish is detected
/// within one actor tick (≤ 250 ms) of its deadline, with no new thread or
/// polling loop.
pub(super) fn sweep_inflight_timeouts(
    in_flight: &mut InFlight,
    now_ms: u64,
    deadline_ms: u64,
    policy: RetryPolicy,
) -> bool {
    let mut changed = false;
    for state in in_flight.per_relay.values_mut() {
        if let PerRelayState::InFlight {
            sent_at_ms,
            attempt,
        } = *state
        {
            if now_ms.saturating_sub(sent_at_ms) >= deadline_ms {
                *state = if attempt >= policy.transient_max_retries {
                    PerRelayState::FailedAfterRetries {
                        reason: format!("timeout after {attempt} retries"),
                        last_at_ms: now_ms,
                    }
                } else {
                    PerRelayState::TimedOut {
                        attempt,
                        last_at_ms: now_ms,
                    }
                };
                changed = true;
            }
        }
    }
    if changed {
        in_flight.dirty = true;
    }
    changed
}

/// Coarse outcome computed from the current per-relay states. Used by the
/// ledger to record a single verdict for the publish.
#[must_use]
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
