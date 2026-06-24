//! Engine projection / terminal-recording side of `PublishEngine`.
//!
//! Extracted from `engine.rs` to keep the orchestrator file under the 500-LOC
//! hand-authored ceiling (AGENTS.md / V-12). These methods are the snapshot
//! plumbing — `flush_view`, `emit_no_targets`, the `record_terminal` family,
//! and the per-tick drain the kernel consumes (`take_pending_terminals` — the
//! single engine→ledger terminal stream). No relay I/O, no retry policy decisions.

use super::super::action::PublishHandle;
use super::super::view::{EventPublishStatus, RecentFailure};
use super::types::LastTerminal;
use super::PublishEngine;
use nmp_signer_iface::SignedEvent;

impl PublishEngine {
    /// Refresh the view's `in_flight` projection. Skips emission unless at
    /// least one row is dirty (or a recently-removed row needs to clear).
    pub(super) fn flush_view(&mut self) {
        let mut any_dirty = self.needs_in_flight_rebuild;
        self.needs_in_flight_rebuild = false;
        let mut in_flight_rows = Vec::new();
        for (handle, row) in &mut self.in_flight {
            any_dirty |= row.dirty;
            row.dirty = false;
            in_flight_rows.push(EventPublishStatus {
                handle: handle.clone(),
                event_id: row.event.id.clone(),
                kind: row.event.unsigned.kind,
                created_at: row.event.unsigned.created_at,
                content: row.event.unsigned.content.clone(),
                per_relay: row
                    .per_relay
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                relay_reasons: row
                    .relay_reasons
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            });
        }
        if !any_dirty {
            return;
        }
        self.view.replace_in_flight(in_flight_rows);
        self.view.bump_rev();
    }

    /// `NoTargets`-path recording: push a `RecentFailure` on the snapshot and
    /// a terminal `"failed"` verdict on `pending_terminals`. Called when the
    /// resolver returned an empty relay set so the publish never reaches the
    /// in-flight map.
    pub(super) fn emit_no_targets(
        &mut self,
        handle: &PublishHandle,
        event: &SignedEvent,
        correlation_id_override: Option<&str>,
        now_ms: u64,
    ) {
        self.view.push_failure(RecentFailure {
            handle: handle.clone(),
            event_id: event.id.clone(),
            relay_url: "(none)".to_string(),
            reason: "no relays resolved for publish target".to_string(),
            at_ms: now_ms,
        });
        // Direction review #24: NoTargets is a terminal "failed" outcome — the
        // publish never gets queued and `start_publish` returns Err(NoTargets),
        // so it never reaches the relay-settlement (`on_ack`) path. Record it
        // here so `action_results` reports the failure and the host clears its
        // spinner instead of waiting on an op that never ran. Its `publish_queue`
        // payload is `None`: the kernel error path pushes the queue row itself.
        //
        // Report the dispatch correlation_id when one was supplied (the
        // `PublishRaw` path), otherwise the handle — same fallback rule as
        // `LastTerminal::from_outcome`.
        self.record_terminal(LastTerminal {
            correlation_id: correlation_id_override.map_or_else(|| handle.clone(), str::to_string),
            status: "failed",
            error: Some("no relays resolved for publish target".to_string()),
            // The event was signed (a handle exists); surface its id so a
            // consumer can still reference the event that failed to reach a
            // relay (#1702). The handle IS the event id for publish actions.
            event_id: Some(handle.clone()),
            result_json: None,
            reason_code: None,
            // `NoTargets` returns `Err` from `start_publish` before any in-flight
            // row; the kernel error path pushes the queue row itself, so this
            // terminal does NOT refine the queue (S11 slice 4, #1758).
            publish_queue: super::types::PublishQueueTerminal::None,
        });
        self.view.bump_rev();
    }

    /// Direction review #29: record one terminal action verdict by appending
    /// to `pending_terminals` (the per-tick drain that fixes the spinner-hang
    /// bug — two settlements in one tick both survive). Every site that
    /// produces a terminal verdict routes through here.
    pub(super) fn record_terminal(&mut self, terminal: LastTerminal) {
        self.pending_terminals.push(terminal);
    }

    // S11 slice 2 (#1758): the off-band terminal recorders
    // (`record_action_terminal_failure` / `record_action_terminal_success`)
    // were deleted. Those sign-step-failure and NWC-success verdicts now record
    // DIRECTLY into the `ActionLedger` (the single source of `action_results`)
    // via `Kernel::record_action_failure_coded` / `record_action_success`, not
    // through the engine `pending_terminals` `Vec`. The engine transport keeps
    // only terminals that ORIGINATE inside the engine (relay ack/tick via
    // `from_outcome`, `emit_no_targets`, cancel).

    /// Direction review #29: drain every terminal verdict recorded since the
    /// last call. The kernel calls this from the snapshot path
    /// (`make_update` → `take_action_results_projection`) so each tick surfaces
    /// every action that settled. Pure drain: after this call the engine
    /// retains no per-tick terminal history.
    #[must_use]
    pub(crate) fn take_pending_terminals(&mut self) -> Vec<LastTerminal> {
        std::mem::take(&mut self.pending_terminals)
    }
}
