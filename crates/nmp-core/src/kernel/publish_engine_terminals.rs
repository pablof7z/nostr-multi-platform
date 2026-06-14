//! Terminal-verdict drains for the kernel ↔ `PublishEngine` wiring.
//!
//! Extracted from `publish_engine.rs` to keep that file under the 500-LOC
//! hand-authored ceiling (AGENTS.md / V-12). This module owns the two
//! per-tick drains the kernel runs against the engine:
//!   - `take_action_results_projection` — the `action_results` projection
//!     edge the host reads to clear per-action spinners.
//!   - `apply_engine_completions` — flips `PublishQueueEntry` rows from
//!     `accepted_locally` to their terminal `"ok"` / `"failed"` status.
//!
//! Plus the free-standing `classify_terminal_outcome` helper that maps a
//! `TerminalOutcome` into the wire-level `(status, outcomes)` pair.

use crate::publish::TerminalOutcome;

use super::super::Kernel;

impl Kernel {
    /// Direction review #29: drain ALL terminals that settled since the last
    /// emit, returning them as a JSON array for the `action_results` snapshot
    /// projection. Each tick surfaces every result that arrived, not just the
    /// most recent. The host uses this to resolve any spinner whose
    /// `correlation_id` appears here.
    ///
    /// As a sibling effect, every terminal also records an `Accepted`
    /// / `Failed` stage into the `action_stages` snapshot mirror so a host
    /// that listens through the stage seam (a richer lifecycle than the
    /// boolean `action_results` drain) observes the terminal exactly once.
    /// The two surfaces are additive: `action_results` is the per-tick edge,
    /// `action_stages` is the persisted mirror. A host may use either.
    pub(in super::super) fn take_action_results_projection(&mut self) -> serde_json::Value {
        let terminals = self.publish_engine.take_pending_terminals();
        // ADR-0055 Rung 1 (F2): drive the drain tristate exactly once per emit.
        // `note_drain_emit` bumps `settlement_drain_ver` only on a non-empty
        // drain (Changed) or on the non-empty -> empty transition (Cleared, so
        // the host drops its prior copy without a replay); a stably-empty drain
        // settles to Unchanged with no churn.
        self.projection_rev_tracker
            .note_drain_emit("action_results", !terminals.is_empty());
        if terminals.is_empty() {
            return serde_json::Value::Null;
        }
        // Record the terminal into the stage mirror *before* serializing
        // the action_results array. The mirror's `at_ms` is sourced from
        // `now_ms()` so a `FixedClock` keeps the timestamp deterministic.
        //
        // V5 thin-shell: route through `record_action_stage` (instead of
        // the bare `action_stages.record`) so the `action_lifecycle`
        // display projection picks up the terminal in the same edge. A
        // host that only consumes `action_lifecycle` now sees engine
        // terminals appear in `recent_terminal` exactly as it sees
        // sign-step terminals from `record_action_failure` /
        // `record_action_success`.
        for terminal in &terminals {
            let stage = match terminal.status {
                "ok" => super::super::action_stages::ActionStage::Accepted,
                _ => super::super::action_stages::ActionStage::Failed {
                    reason: terminal
                        .error
                        .clone()
                        .unwrap_or_else(|| terminal.status.to_string()),
                },
            };
            // `record_action_stage` is silent on cap hits (D6) — the
            // diagnostic counters in the underlying trackers surface the
            // event without interrupting the publish path.
            self.record_action_stage(&terminal.correlation_id, stage, None);
        }
        let arr: Vec<serde_json::Value> = terminals
            .iter()
            .map(|terminal| {
                let status = match terminal.status {
                    "ok" => "published",
                    other => other,
                };
                let mut row = serde_json::json!({
                    "correlation_id": terminal.correlation_id,
                    "status": status,
                    "error": terminal.error,
                });
                // ADR-0043 Decision 4 — forward the opaque structured result
                // body verbatim under `result` when the action attached one.
                // The string is re-parsed into a `serde_json::Value` purely so
                // the host reads a JSON object (not a JSON-encoded string); this
                // is forwarding, NOT interpretation — `nmp-core` learns no
                // protocol noun (D0). A non-JSON body is forwarded as a raw
                // string rather than dropped.
                if let Some(result_json) = &terminal.result_json {
                    let value = serde_json::from_str::<serde_json::Value>(result_json)
                        .unwrap_or_else(|_| serde_json::Value::String(result_json.clone()));
                    if let Some(obj) = row.as_object_mut() {
                        obj.insert("result".to_string(), value);
                    }
                }
                row
            })
            .collect();
        serde_json::Value::Array(arr)
    }

    /// T128: drain every terminal verdict the engine recorded since the last
    /// drain and flip the matching `PublishQueueEntry` from `accepted_locally`
    /// to its terminal `"ok"` / `"failed"` status, carrying the per-relay
    /// outcome map. Called after every engine entrypoint
    /// (`run_publish_engine_at`, `handle_publish_ok_at`, `tick_publish_engine`,
    /// `resume_publish_engine`).
    ///
    /// Status mapping (per the iOS UX requirement — partial success is still
    /// surfaced under the `"ok"` branch with N/M detail):
    /// - `accepted.is_empty() && !failed.is_empty()` → `"failed"`
    /// - any accepted (with or without failures) → `"ok"`
    /// - both empty → `"failed"` defensively (no relays settled at all)
    pub(in super::super) fn apply_engine_completions(&mut self) {
        let completions = self.publish_engine.take_completed();
        if completions.is_empty() {
            return;
        }
        for outcome in completions {
            let (status, outcomes) = classify_terminal_outcome(&outcome);
            self.set_publish_entry_terminal(&outcome.event_id, status, outcomes);
            // V-18: surface a user-visible toast when every relay returned
            // `FailedAfterRetries`. Without this, a post that no relay
            // accepted would silently sit in the Outbox with no feedback to
            // the user. `classify_terminal_outcome` already maps the
            // empty-accepted case to `"failed"`, so we trust the helper. The
            // `NoTargets` / pre-sign-step path is handled separately by
            // `record_engine_error`.
            if status == "failed" {
                self.set_last_error_toast(Some(
                    "Couldn't reach any relay — your post is in the Outbox".to_string(),
                ));
            }
        }
        // `changed_since_emit` is set inside `set_publish_entry_terminal` on
        // any field change; setting again here is redundant but documents the
        // intent (terminal transitions are always snapshot-worthy).
        self.changed_since_emit = true;
    }
}

/// T128: map a `TerminalOutcome` into the wire-level `(status, outcomes)`
/// pair. Kept free-standing so the kernel tests can assert the contract
/// without going through `apply_engine_completions`.
fn classify_terminal_outcome(
    outcome: &TerminalOutcome,
) -> (&'static str, Vec<super::super::RelayAckOutcome>) {
    use super::super::publish_outbox::format_relay_reasons;
    let mut outcomes = Vec::with_capacity(outcome.accepted.len() + outcome.failed.len());
    for url in &outcome.accepted {
        // Look up the per-relay rationale captured at publish time so the
        // settled queue entry carries the same "why was this relay targeted?"
        // string the in-flight outbox shows. Missing entries fall back to
        // empty (older serialised rows / resumes never wrote the map).
        let relay_reason = outcome
            .relay_reasons
            .get(url)
            .map(|reasons| format_relay_reasons(reasons))
            .unwrap_or_default();
        outcomes.push(super::super::RelayAckOutcome {
            relay_url: url.clone(),
            status: "ok".to_string(),
            message: String::new(),
            relay_reason,
        });
    }
    for (url, reason) in &outcome.failed {
        let relay_reason = outcome
            .relay_reasons
            .get(url)
            .map(|reasons| format_relay_reasons(reasons))
            .unwrap_or_default();
        outcomes.push(super::super::RelayAckOutcome {
            relay_url: url.clone(),
            status: "failed".to_string(),
            message: reason.clone(),
            relay_reason,
        });
    }
    let status = if outcome.accepted.is_empty() {
        // Pure failure — every relay reached FailedAfterRetries. (NoTargets
        // never reaches this path; it's handled in `run_publish_engine_at`
        // via `record_engine_error`.)
        "failed"
    } else {
        // At least one Ok — partial-success and full-success both report
        // `"ok"`; the per-relay detail tells iOS whether it's N/M or N/N.
        "ok"
    };
    (status, outcomes)
}
