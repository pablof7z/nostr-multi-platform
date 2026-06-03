//! Engine projection / terminal-recording side of `PublishEngine`.
//!
//! Extracted from `engine.rs` to keep the orchestrator file under the 500-LOC
//! hand-authored ceiling (AGENTS.md / V-12). These methods are the snapshot
//! plumbing — `flush_view`, `emit_no_targets`, the `record_terminal` family,
//! and the per-tick drains the kernel consumes (`take_completed`,
//! `take_pending_terminals`). No relay I/O, no retry policy decisions.

use super::super::action::PublishHandle;
use super::super::view::{EventPublishStatus, RecentFailure};
use super::types::{LastTerminal, TerminalOutcome};
use super::PublishEngine;
use crate::substrate::SignedEvent;

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
        // so it never reaches the `recently_completed` / `on_ack` paths.
        // Record it here so `action_results` reports the failure and the
        // host clears its spinner instead of waiting on an op that never ran.
        //
        // Report the dispatch correlation_id when one was supplied (the
        // `PublishRaw` path), otherwise the handle — same fallback rule as
        // `LastTerminal::from_outcome`.
        self.record_terminal(LastTerminal {
            correlation_id: correlation_id_override.map_or_else(|| handle.clone(), str::to_string),
            status: "failed",
            error: Some("no relays resolved for publish target".to_string()),
            result_json: None,
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

    /// Record a terminal `"failed"` verdict for a dispatched action that never
    /// reached the publish engine's in-flight set — the event was never signed,
    /// so there is no `PublishHandle` and no `TerminalOutcome`.
    ///
    /// This closes a broken-promise gap: a host that dispatched a
    /// `PublishRaw` / `PublishProfile` through `nmp_app_dispatch_action`
    /// received a registry-minted `correlation_id` and is waiting to see its
    /// outcome in the `action_results` snapshot projection. When the *sign*
    /// step fails (no active account, a malformed reply id, a local-key sign
    /// error, or a remote-signer timeout / rejection) the publish never
    /// happens — without this entry the host's spinner keyed on that
    /// `correlation_id` would hang forever.
    ///
    /// Unlike `record_engine_error` this does **not** push a `RecentFailure`
    /// row: no event/handle exists to anchor one, and the caller already
    /// surfaces a `set_last_error_toast`. This records *only* the
    /// `action_results` terminal so the dispatched action's promise is
    /// honoured.
    pub(crate) fn record_action_terminal_failure(&mut self, correlation_id: String, error: String) {
        self.record_terminal(LastTerminal {
            correlation_id,
            status: "failed",
            error: Some(error),
            result_json: None,
        });
    }

    /// Record a terminal `"ok"` verdict for a dispatched action that completed
    /// **without** going through the publish-engine in-flight set — i.e. the
    /// outcome is observed off-band, not via a relay OK on a signed event.
    ///
    /// The motivating consumer is NIP-47 NWC `pay_invoice`: the action's
    /// terminal outcome is the **wallet's** kind:23195 response carrying a
    /// `preimage`. That response never reaches the publish engine (the
    /// kind:23194 request itself settles separately as a normal publish; the
    /// *payment* outcome lives in the NWC response channel), so a host that
    /// dispatched the payment through `nmp_app_dispatch_action` would
    /// otherwise have no `action_results` entry to drain its spinner — the
    /// same broken-promise gap `record_action_terminal_failure` closes for
    /// sign-step failures.
    ///
    /// Mirrors `record_action_terminal_failure`: pushes a single
    /// `LastTerminal { status: "ok", error: None }` onto `pending_terminals`
    /// for the next snapshot drain. No `RecentFailure` row is written (success
    /// paths don't anchor failure rows); the caller is responsible for any
    /// projection-level state (e.g. wallet balance refresh) it needs.
    ///
    /// `result_json` (ADR-0043 Decision 4) is an opaque structured result body
    /// the action attaches to its success terminal — forwarded verbatim into
    /// the `action_results` row's `result` field. `nmp-core` NEVER parses it.
    /// `None` for the NWC pay-invoice path; `Some(json)` for a protocol crate
    /// (e.g. a Blossom blob descriptor) that carries a return payload.
    // `#[allow(dead_code)]`: the live callers are `Kernel::record_action_success`
    // (publish_cmd.rs, gated behind the `wallet` feature for NWC) and the
    // `RecordActionSuccess { result_json }` dispatch arm. A plain
    // `cargo check -p nmp-core` (default features) may not see a consumer, and
    // the per-crate dead-code lint can fire; the cross-feature truth is
    // invisible to rustc here.
    #[allow(dead_code)]
    pub(crate) fn record_action_terminal_success(
        &mut self,
        correlation_id: String,
        result_json: Option<String>,
    ) {
        self.record_terminal(LastTerminal {
            correlation_id,
            status: "ok",
            error: None,
            result_json,
        });
    }

    /// T128: drain every terminal verdict recorded since the last call. The
    /// kernel calls this after every engine entrypoint (`start_publish` /
    /// `on_ack` / `tick` / `resume_from_store`) and applies the verdicts to
    /// its `PublishQueueEntry` projection. Pure drain — the engine retains no
    /// per-publish history after this call (the snapshot's `recent_ok` /
    /// `recent_errors` carry the longer view).
    #[must_use]
    pub(crate) fn take_completed(&mut self) -> Vec<TerminalOutcome> {
        std::mem::take(&mut self.recently_completed)
            .into_values()
            .collect()
    }

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
