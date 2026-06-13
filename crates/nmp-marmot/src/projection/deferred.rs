//! Deferred KP-gated op management for [`super::state::InnerHandle`].
//!
//! Split out of `state.rs` / `ops.rs` (LOC ceiling) as a cohesive
//! sub-concern: parking ops blocked on missing peer KeyPackages, retrying
//! them when the KP arrives, expiring them on a wall-clock edge, and recording
//! the terminal verdict. See [`super::pending::PendingOpsStore`] for the
//! underlying store and its expiry / single-flight rules.
//!
//! This is a continuation `impl InnerHandle` block — it reaches into the
//! `pub(super)` `inner` field and the `pub(super)` `app()` accessor on
//! `InnerHandle`, both declared in `state.rs`.

use nostr::PublicKey;
use serde_json::{json, Value};

use super::pending::{PendingOp, RetryOutcome, StoreResult};
use super::state::InnerHandle;

impl InnerHandle<'_> {
    // ── Pending-op management (deferred KP-gated ops) ───────────────────

    /// When a KP-gated op hits `key_package_unavailable` AND a `correlation_id`
    /// is available (typed dispatch pipeline), park the op and return a
    /// `{"pending":true}` envelope instead of a terminal failure. The
    /// `DispatchHostOp` arm reads `{"pending":true}` as "leave the action in
    /// `Requested`; the handler records the terminal verdict later". Without a
    /// `correlation_id` (REPL / tests) fall back to the old `{"ok":false}`.
    ///
    /// Single-flight: an identical op + missing-pubkeys fingerprint already
    /// parked is rejected, returning a `{"pending":true,"duplicate":true}`
    /// referencing the already-pending `correlation_id` — no double-create.
    pub(crate) fn park_or_report_kp_unavailable(
        &mut self,
        action_value: &Value,
        op_tag: &str,
        needs: Vec<String>,
        fetch_pubkeys: &[PublicKey],
        correlation_id: Option<&str>,
        now_secs: u64,
    ) -> Value {
        // Always fire the fetch interest (idempotent at the planner level).
        let fetch_requested = self.request_key_package_fetch(fetch_pubkeys);
        let needs_pubkeys_hex: Vec<String> =
            fetch_pubkeys.iter().map(PublicKey::to_hex).collect();

        let Some(cid) = correlation_id else {
            // No correlation_id → outside the typed pipeline (REPL / tests):
            // fall back to the old terminal soft-fail (no spinner to keep alive).
            return json!({
                "ok": false,
                "error": "key_package_unavailable",
                "needs": needs,
                "needs_pubkeys_hex": needs_pubkeys_hex,
                "fetch_requested": fetch_requested,
                "hint": "key package lookup was requested; results arrive via the kernel tap",
            });
        };

        let action_json = serde_json::to_string(action_value)
            .unwrap_or_else(|_| action_value.to_string());
        let store_result = self.park_pending_op(
            cid.to_string(),
            action_json,
            op_tag,
            needs_pubkeys_hex.clone(),
            now_secs,
        );
        match store_result {
            StoreResult::Stored => json!({
                "pending": true,
                "correlation_id": cid,
                "needs_pubkeys_hex": needs_pubkeys_hex,
                "fetch_requested": fetch_requested,
            }),
            StoreResult::Duplicate { existing_correlation_id } => json!({
                "pending": true,
                "duplicate": true,
                "correlation_id": existing_correlation_id,
                "needs_pubkeys_hex": needs_pubkeys_hex,
                "fetch_requested": fetch_requested,
            }),
        }
    }

    /// Park a KP-blocked op in the pending store. Returns the [`StoreResult`]
    /// so the caller can surface "pending" or "duplicate pending".
    /// `missing_pubkeys_hex` must NOT be empty.
    pub(crate) fn park_pending_op(
        &mut self,
        correlation_id: String,
        action_json: String,
        op_tag: &str,
        missing_pubkeys_hex: Vec<String>,
        now_secs: u64,
    ) -> StoreResult {
        self.inner.pending_ops.store(
            correlation_id,
            action_json,
            op_tag,
            missing_pubkeys_hex,
            now_secs,
        )
    }

    /// After a KP event for `pubkey_hex` is cached, check whether any pending
    /// ops are now unblocked. Returns ready ops as [`RetryOutcome`]s and
    /// removes them from the store (caller re-dispatches + records the
    /// verdict). Also evicts expired ops (wall-clock gate; `now_secs` always
    /// caller-supplied, never a timer).
    pub(crate) fn handle_key_package_cached(
        &mut self,
        pubkey_hex: &str,
        now_secs: u64,
    ) -> Vec<RetryOutcome> {
        self.evict_expired_pending(now_secs);
        self.inner.pending_ops.retry_for_pubkey(pubkey_hex)
    }

    /// Re-dispatch every op unblocked by `pubkey_hex` (and expire stale ones),
    /// recording the terminal verdict under each op's ORIGINAL `correlation_id`
    /// via the actor channel. Called from the kind:443/30443 ingest arm after
    /// the KP is cached. Fires exactly once per KP arrival (D8 — no polling).
    pub(crate) fn retry_unblocked_ops(&mut self, pubkey_hex: &str, now_secs: u64) {
        let ready = self.handle_key_package_cached(pubkey_hex, now_secs);
        for outcome in ready {
            // Re-run the original op through the full dispatch path. The
            // pending store only returns an op once its missing set is empty,
            // so the re-dispatch should not re-park; `{"pending":true}` is
            // handled defensively (skip the terminal write).
            let op_value: Value = serde_json::from_str(&outcome.action_json)
                .unwrap_or_else(|_| json!({ "op": "__invalid__" }));
            let result =
                super::ops::dispatch(self, &op_value, now_secs, Some(&outcome.correlation_id));
            let ok = result.get("ok").and_then(Value::as_bool);
            let pending = result.get("pending").and_then(Value::as_bool).unwrap_or(false);
            if pending {
                continue;
            }
            let cmd = if ok == Some(true) {
                nmp_core::ActorCommand::RecordActionSuccess {
                    correlation_id: outcome.correlation_id,
                    result_json: None,
                }
            } else {
                let reason = result
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("deferred op failed")
                    .to_string();
                nmp_core::ActorCommand::RecordActionFailure {
                    correlation_id: outcome.correlation_id,
                    reason,
                }
            };
            self.push_actor_command(cmd);
        }
    }

    /// Evict expired pending ops and push terminal failure commands for each.
    /// Driven from BOTH wall-clock edges — the KP-ingest arm (via
    /// [`Self::handle_key_package_cached`]) and the top of
    /// [`super::state::MarmotProjection::snapshot`] — so an op whose KP never
    /// arrives still ages out within a tick of its deadline. No live actor
    /// channel (test path) → the `RecordActionFailure` send is a no-op, but the
    /// verdict is still recorded in the `#[cfg(test)]` capture seam.
    pub(super) fn evict_expired_pending(&mut self, now_secs: u64) {
        let expired: Vec<PendingOp> = self.inner.pending_ops.check_expired(now_secs);
        for op in expired {
            self.push_actor_command(nmp_core::ActorCommand::RecordActionFailure {
                correlation_id: op.correlation_id,
                reason: "key_package_unavailable".to_string(),
            });
        }
    }

    /// Send an [`nmp_core::ActorCommand`] back into the actor's own command
    /// channel. D8-safe — the unbounded `mpsc::Sender::send` is non-blocking;
    /// the actor drains it next iteration. No-op when `app` is null (test
    /// projection — no actor channel).
    pub(crate) fn push_actor_command(&mut self, cmd: nmp_core::ActorCommand) {
        // Test capture seam: record a lightweight `(verdict, correlation_id)`
        // projection of the command stream so unit tests can assert EXACTLY
        // ONE terminal verdict per correlation_id without a live `NmpApp`.
        // `ActorCommand` is not `Clone`, so we project the two fields the test
        // cares about before the move-by-value send. Compiled out in prod.
        #[cfg(test)]
        {
            match &cmd {
                nmp_core::ActorCommand::RecordActionSuccess { correlation_id, .. } => {
                    self.inner
                        .captured_commands
                        .push(("success", correlation_id.clone()));
                }
                nmp_core::ActorCommand::RecordActionFailure { correlation_id, .. } => {
                    self.inner
                        .captured_commands
                        .push(("failure", correlation_id.clone()));
                }
                _ => {}
            }
        }

        let Some(app) = self.app() else {
            return;
        };
        let _ = app.actor_sender().send(cmd);
    }

    /// Snapshot view: collect pending op descriptors as
    /// `(correlation_id, op_tag, missing_count)`, consumed by tests.
    #[must_use]
    #[allow(dead_code)] // consumed by deferred-op tests.
    pub(crate) fn pending_op_summaries(&self) -> Vec<(String, String, usize)> {
        self.inner
            .pending_ops
            .iter()
            .map(|op| {
                let op_tag = serde_json::from_str::<Value>(&op.action_json)
                    .ok()
                    .and_then(|v| v.get("op").and_then(|s| s.as_str()).map(str::to_owned))
                    .unwrap_or_default();
                (op.correlation_id.clone(), op_tag, op.missing_pubkeys.len())
            })
            .collect()
    }

    /// Test-only: drain the captured `(verdict, correlation_id)` command stream
    /// recorded by [`Self::push_actor_command`]. Lets a test assert the EXACT
    /// terminal-verdict sequence (one per correlation_id) across retry-success
    /// / expiry / expiry-then-late-KP flows without a live `NmpApp`.
    #[cfg(test)]
    pub(crate) fn drain_captured_commands(&mut self) -> Vec<(&'static str, String)> {
        std::mem::take(&mut self.inner.captured_commands)
    }
}
