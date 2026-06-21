//! Action-dispatch arm of [`super::WasmRuntime::handle`].
//!
//! Split out of `runtime.rs` (LOC ceiling) — the synchronous `dispatch`
//! router plus its two helpers (`app_action`, `accepted_with_snapshot`) are a
//! cohesive unit: they translate a host `ActionDispatch` / `AppAction` into the
//! `KernelReducer` mutation + the `[ActionAccepted, UpdateBytes?]` reply. The
//! relay-driven snapshot push and the `Start`/`Stop`/`SetSigner` arms stay in
//! `runtime.rs`; only the action-namespace routing lives here.
//!
//! The methods are defined on `impl super::WasmRuntime` so they remain ordinary
//! private methods of the runtime — the file boundary is a size-management
//! seam, not an API boundary.

use crate::dispatch_routing::{
    claim_dispatch_from_action, execute_claim_dispatch, execute_interest_dispatch,
    interest_dispatch_from_action, kernel_action_from_dispatch, write_path_unavailable_reason,
};
use crate::protocol::{ActionDispatch, AppAction, CapabilityFailure, WorkerEvent};
use nmp_core::KernelUpdate;

use super::{WasmRuntime, WasmRuntimeError};

impl WasmRuntime {
    /// Build an `[ActionAccepted, UpdateBytes]` pair for a successful
    /// synchronous dispatch. Used by every arm that fans outbound and then
    /// returns the standard acknowledgement + snapshot.
    pub(super) fn accepted_with_snapshot(
        &mut self,
        action_type: String,
        correlation_id: String,
    ) -> Vec<WorkerEvent> {
        vec![
            WorkerEvent::ActionAccepted { action_type, correlation_id },
            self.snapshot_event(),
        ]
    }

    /// Drain one older page for the feed registered under `feed_key` (ADR-0058).
    ///
    /// The wasm twin of `NmpApp::load_older_feed`: forwards to the feed's
    /// `PullFeedController` (which re-reads the live, fail-closed interest shape,
    /// runs a bounded seq-ordered pull drain over the kernel event store,
    /// ingests the page through the feed's own observer path, and grows the
    /// render viewport). The shell does NO pull/cursor logic.
    ///
    /// On a non-empty drain the parked claim queue is flushed (so newly-pulled
    /// rows resolve their kind:0 profiles) and a fresh snapshot is emitted so
    /// the grown projection re-renders. A no-op drain (fail-closed shape /
    /// exhausted log) returns only the `ActionAccepted` — no wasted frame.
    pub(super) fn load_older_feed(
        &mut self,
        feed_key: &str,
        correlation_id: String,
    ) -> Vec<WorkerEvent> {
        let grew = self.feed_registry.load_older(feed_key);
        let mut events = vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.feed.load_older".to_string(),
            correlation_id,
        }];
        if grew {
            // The controller's `apply` (engine observer) may have parked claim
            // requests for newly-surfaced authors; flush them through the same
            // post-tick drain the relay-ingest path uses (the registry borrow is
            // already released, so `reducer.borrow_mut()` inside the drain is
            // re-entrancy-safe). Then push the grown `nmp.feed.home` projection.
            let drain = self.post_tick_drain.borrow().clone();
            if let Some(drain) = drain {
                drain();
            }
            events.push(self.snapshot_event());
        }
        events
    }

    pub(super) fn app_action(
        &mut self,
        action: AppAction,
        correlation_id: String,
    ) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        let (action_type, _payload) = action.into_dispatch_parts();
        Ok(vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action_type,
            correlation_id,
            reason: write_path_unavailable_reason(self.signer.as_ref()),
        })])
    }

    pub(super) fn dispatch(
        &mut self,
        action: ActionDispatch,
    ) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        // F-CR-00 claim arm: claim/release refcounts (see execute_claim_dispatch
        // in dispatch_routing.rs for the full rationale / `can_send` contract).
        if let Some(claim) = claim_dispatch_from_action(&action) {
            let can_send = self.reducer.borrow().any_relay_connected();
            let outbound = execute_claim_dispatch(&mut self.reducer.borrow_mut(), claim, can_send);
            self.fan_outbound(outbound);
            // Claim/release are refcount bookkeeping — they carry no new
            // user-visible data of their own (the resolved kind:0 arrives later
            // via the relay-pool ingest sink, which pushes its OWN snapshot).
            // Pushing a snapshot here hands the reactive web host a fresh frame
            // on every claim; the host's feed `<For>` rebuilds its rows, which
            // remounts the avatar/name components, which release + re-claim —
            // an unbounded claim → snapshot → re-render → claim loop that, on
            // the single-threaded wasm worker, floods the main thread with
            // snapshot frames and starves (or OOM-crashes) the UI so the feed
            // never paints (feed.spec.ts toBeVisible timeout). Only ACK the
            // action; let the data-bearing ingest frame drive the next render.
            return Ok(vec![WorkerEvent::ActionAccepted {
                action_type: action.action_type,
                correlation_id: action.correlation_id,
            }]);
        }
        // PR-3 feed-verb arm: open/close generic interests + contact-feed.
        if let Some(interest) = interest_dispatch_from_action(&action) {
            let outbound = execute_interest_dispatch(&mut self.reducer.borrow_mut(), interest);
            self.fan_outbound(outbound);
            return Ok(self.accepted_with_snapshot(action.action_type, action.correlation_id));
        }
        // Kernel-namespace actions (`nmp.kernel.start`, `open_uri`, etc.) map
        // to `KernelAction` variants and run through `KernelReducer::reduce`.
        if let Some(kernel_action) = kernel_action_from_dispatch(&action) {
            let update = self.reducer.borrow_mut().reduce(kernel_action);
            match update {
                KernelUpdate::Started { .. } => { self.meta.borrow_mut().started = true; }
                KernelUpdate::Stopped { .. } => { self.meta.borrow_mut().started = false; }
                _ => {}
            }
            return Ok(self.accepted_with_snapshot(action.action_type, action.correlation_id));
        }
        Ok(vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action.action_type,
            correlation_id: action.correlation_id,
            reason: write_path_unavailable_reason(self.signer.as_ref()),
        })])
    }
}
