//! Terminal-verdict drains for the kernel ↔ `PublishEngine` wiring.
//!
//! Extracted from `publish_engine.rs` to keep that file under the 500-LOC
//! hand-authored ceiling (AGENTS.md / V-12). This module owns the two
//! per-tick drains and projection-source sync the kernel runs against the engine:
//!   - `take_action_results_projection` — the `action_results` projection
//!     edge the host reads to clear per-action spinners.
//!   - `apply_engine_completions` — flips `PublishQueueEntry` rows from
//!     `accepted_locally` to their terminal `"ok"` / `"failed"` status.
//!   - `bump_publish_if_engine_view_changed` — keeps ADR-0055's `publish_engine_ver`
//!     aligned with the engine-owned in-flight view used by `publish_outbox`.
//!
//! Plus the free-standing `classify_terminal_outcome` helper that maps a
//! `TerminalOutcome` into the wire-level `(status, outcomes)` pair.

use crate::publish::TerminalOutcome;

use super::super::Kernel;

impl Kernel {
    /// ADR-0055 / #1412: `publish_outbox` and `outbox_summary` derive from
    /// `publish_engine.snapshot().in_flight`, not just `publish_queue`. Whenever
    /// an engine entrypoint advances its view rev, the publish-engine source
    /// counter must advance too or Rung 3 can omit a changed outbox payload.
    pub(in super::super) fn bump_publish_if_engine_view_changed(&mut self, before_rev: u64) {
        if self.publish_engine.snapshot().rev != before_rev {
            self.projection_rev_tracker
                .source_versions
                .bump_publish_engine();
        }
    }

    /// Fold every terminal the engine produced since the last fold into the
    /// `ActionLedger` — the SINGLE source of `action_results` (S11 slice 2,
    /// #1758). Called at every kernel↔engine boundary (alongside
    /// `apply_engine_completions`, plus the `NoTargets` and cancel paths that
    /// bypass it) so an engine-origin terminal lands in the ledger at the moment
    /// it is PRODUCED — the same instant an off-band terminal
    /// (`record_action_failure_coded` / `record_action_success`) records itself.
    /// That preserves the chronological producer order across mixed terminal
    /// sources within a tick (byte-identical to the prior single engine `Vec`),
    /// rather than deferring engine terminals to emit time.
    ///
    /// Maps the engine's internal status vocabulary to the WIRE status + the
    /// substrate stage in one place, and threads the curated `reason_code` (#1735)
    /// into the `action_lifecycle` view. Pure drain of `pending_terminals` — a
    /// boundary with no settled terminal is a cheap no-op (empty `Vec`).
    pub(in super::super) fn drain_engine_terminals_into_ledger(&mut self) {
        let pending = self.publish_engine.take_pending_terminals();
        if pending.is_empty() {
            return;
        }
        // The mirror's `at_ms` is sourced from `now_ms()` so a `FixedClock`
        // keeps the timestamp deterministic.
        let now_ms = self.now_ms();
        for terminal in pending {
            // S7 (#1754): a user-initiated cancel is the DISTINCT `Cancelled`
            // terminal, never `Failed`. The engine records the cancel terminal
            // with `status == "cancelled"` (see `PublishEngine::cancel_by_handle`)
            // under the ORIGINAL correlation_id; this is the single path that
            // mirrors it into `action_stages` / `action_lifecycle`.
            let (stage, status) = match terminal.status {
                "ok" => (
                    super::super::action_stages::ActionStage::Accepted,
                    "published",
                ),
                "cancelled" => (
                    super::super::action_stages::ActionStage::Cancelled,
                    "cancelled",
                ),
                _ => (
                    super::super::action_stages::ActionStage::Failed {
                        reason: terminal
                            .error
                            .clone()
                            .unwrap_or_else(|| terminal.status.to_string()),
                    },
                    "failed",
                ),
            };
            // #1735: thread the curated `reason_code` (e.g. the D10 routing-leak
            // refusal) into the `action_lifecycle` projection. `record_terminal`
            // is silent on cap hits (D6) — the diagnostic counters in the
            // underlying trackers surface the event without interrupting the
            // publish path.
            self.action_ledger.record_terminal(
                &terminal.correlation_id,
                stage,
                status,
                terminal.error,
                terminal.result_json,
                terminal.event_id,
                terminal.reason_code,
                None,
                now_ms,
            );
            // A terminal verdict is always snapshot-worthy. Bump the enqueue
            // source version so the `action_stages` / `action_lifecycle`
            // projections (which depend on it) re-serialise on the next emit —
            // the same bump the off-band ledger-record paths perform.
            self.changed_since_emit = true;
            self.projection_rev_tracker
                .source_versions
                .bump_settlement_enqueue();
        }
    }

    /// Drain ALL terminals that settled since the last emit, returning them as a
    /// JSON array for the `action_results` snapshot projection. Each tick
    /// surfaces every result that arrived, not just the most recent. The host
    /// uses this to resolve any spinner whose `correlation_id` appears here.
    ///
    /// S11 slice 2 (#1758): the ledger is the SINGLE source of these rows. Engine-
    /// origin terminals (relay ack/tick, NoTargets, cancel) were already folded
    /// into the ledger at their production boundary by
    /// [`Self::drain_engine_terminals_into_ledger`]; off-band terminals (sign-step
    /// failures, NWC successes) recorded into the ledger directly. This method is
    /// therefore a pure drain of the ledger's per-tick terminal buffer via
    /// [`ActionLedger::take_terminal_results`] — there is no second source to
    /// serialise. (A defensive final fold catches any boundary that produced a
    /// terminal without going through `apply_engine_completions`; it is a no-op
    /// in steady state.)
    pub(in super::super) fn take_action_results_projection(&mut self) -> serde_json::Value {
        // Defensive: fold any straggler engine terminal so no verdict is
        // stranded if a boundary missed the production-time fold. A no-op when
        // `pending_terminals` is already empty (the common case).
        self.drain_engine_terminals_into_ledger();
        // Drain the ledger's per-tick terminal buffer — the single source.
        let rows = self.action_ledger.take_terminal_results();
        // ADR-0055 Rung 1 (F2): drive the drain tristate exactly once per emit.
        // `note_drain_emit` bumps `settlement_drain_ver` only on a non-empty
        // drain (Changed) or on the non-empty -> empty transition (Cleared, so
        // the host drops its prior copy without a replay); a stably-empty drain
        // settles to Unchanged with no churn.
        self.projection_rev_tracker
            .note_drain_emit("action_results", !rows.is_empty());
        if rows.is_empty() {
            return serde_json::Value::Null;
        }
        serde_json::Value::Array(rows)
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
        // S11 slice 2 (#1758): fold any engine-origin `action_results` terminal
        // into the ledger at this engine boundary — at PRODUCTION time, not at
        // emit time — so its producer order is preserved relative to off-band
        // terminals. This is the `action_results` (single-source) lane, distinct
        // from the `recently_completed` publish_queue lane drained below. It must
        // run BEFORE the early return: a `pending_terminals` verdict
        // (e.g. NoTargets, cancel, a relay settlement) can exist independently of
        // a `recently_completed` row.
        self.drain_engine_terminals_into_ledger();
        let completions = self.publish_engine.take_completed();
        if completions.is_empty() {
            return;
        }
        for outcome in completions {
            let (status, outcomes) = classify_terminal_outcome(&outcome);
            self.set_publish_entry_terminal(&outcome.event_id, status, outcomes);
            // S7 (#1754) D8 — forget the handle↔correlation index entry now
            // that this publish has reached a terminal outcome (ok or failed).
            // Mirrors the cancel/clear paths in `publish_outbox.rs` (lines
            // :175/:190). Every terminal path (success, failure, cancel) must
            // forget exactly once; non-terminal stage updates (publishing /
            // awaiting_capability) must NOT forget so the index tracks only
            // the live in-flight set. Without this, a completed publish leaves
            // a stale handle↔correlation entry bounded only by the cap, not
            // by the actual in-flight set — a D8 violation.
            self.publish_handle_correlation.forget(&outcome.event_id);
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
