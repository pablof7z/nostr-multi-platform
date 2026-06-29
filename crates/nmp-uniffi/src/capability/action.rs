//! Action-lane UniFFI methods — M14-C4.
//!
//! Mirrors `nmp-ffi/src/action.rs` for the `ack_action_stage` and
//! `register_action_result_observer` symbols.
//!
//! ## Quiescence for `register_action_result_observer` (M14-C-tail / #2429)
//!
//! `ActionRegistry` now holds the result observer behind a
//! `ResultObserverGate` — the same `in_flight` + `Condvar` drain used by
//! `UpdateListenerGate` / `CapabilityCallbackGate`, instead of holding the
//! `Arc<Mutex>` lock ACROSS the observer call. The UniFFI binding therefore:
//! * Supports registration AND replacement with a drain-before-return
//!   guarantee: `register_action_result_observer` waits for any in-flight
//!   delivery of the previous observer to finish, so the previous callback ARC
//!   may be released the instant it returns.
//! * Exposes `clear_action_result_observer` — the teardown counterpart that was
//!   missing under the old mutex-exclusion scheme. After it returns the
//!   observer is neither registered nor mid-invocation.
//!
//! ## Threading
//!
//! * `ack_action_stage` sends `ActorCommand::ActionLedger(Ack(...))` down the
//!   actor channel (non-blocking, D8).
//! * `register_action_result_observer` / `clear_action_result_observer` swap the
//!   gate's observer slot and drain in-flight delivery before returning (the
//!   only blocking is the bounded wait for a concurrent delivery to complete).

use std::sync::Arc;

use nmp_core::substrate::ActionResult;

use crate::NmpApp;
use super::ActionResultObserver;

#[uniffi::export]
impl NmpApp {
    /// Acknowledge a terminal action stage, removing it from the
    /// `action_stages` snapshot projection.
    ///
    /// The kernel projects `action_stages` (a `correlation_id →
    /// [StageEntry…]` map) on every tick. Unlike `action_results` (which
    /// drain on emit), the same entry reappears every tick until the host
    /// calls this method. Call it after the UI has consumed the terminal
    /// stage (`Accepted` / `Failed`) to drop the entry.
    ///
    /// An empty `correlation_id` or an unknown id is a silent no-op (D6 —
    /// never a crash). D8: non-blocking channel send.
    pub fn ack_action_stage(&self, correlation_id: String) {
        if correlation_id.is_empty() {
            return;
        }
        self.inner.send_cmd(nmp_core::actor::ActorCommand::ActionLedger(
            nmp_core::actor::ActionLedgerCommand::Ack(correlation_id),
        ));
    }

    /// Register a host-supplied action-result observer — the *push*
    /// counterpart to the snapshot-projection (pull) output seam.
    ///
    /// After `dispatch_action` validates an action and its executor returns
    /// `Ok`, the registry calls `on_action_result` with a JSON string
    /// `{"correlation_id":"…","result_json":…}`. For built-in
    /// (fire-and-forget) executors `result_json` is `null`; the signal means
    /// the action was *accepted and enqueued*, not that the actor has finished
    /// publishing.
    ///
    /// A second registration replaces the first and WAITS for any in-flight
    /// delivery of the previous observer to drain before returning (M14-C-tail /
    /// #2429), so the previous callback ARC may be released immediately. Use
    /// [`Self::clear_action_result_observer`] to unregister.
    pub fn register_action_result_observer(&self, observer: Box<dyn ActionResultObserver>) {
        let observer: Arc<dyn ActionResultObserver> = Arc::from(observer);
        self.inner
            .register_action_result_observer(move |result: ActionResult| {
                // Serialize to JSON, matching the C-ABI callback shape:
                // {"correlation_id":"…","result_json":…}
                let Ok(json) = serde_json::to_string(&result) else {
                    return; // D6: serialisation failure is a silent drop
                };
                // Panic containment: a Swift/Kotlin throw must not unwind
                // into the dispatch thread (D6). Clone Arc before the call.
                let o = Arc::clone(&observer);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    o.on_action_result(json);
                }));
            });
    }

    /// Unregister the action-result observer, draining any in-flight delivery
    /// before returning (M14-C-tail / #2429).
    ///
    /// Idempotent: clearing when none is registered is a no-op. After this
    /// returns the observer is neither registered nor mid-invocation, so its
    /// callback ARC may be released — the teardown counterpart that the C4
    /// mutex-exclusion scheme lacked.
    ///
    /// Re-entrancy is forbidden: calling this from inside `on_action_result`
    /// deadlocks the drain gate.
    pub fn clear_action_result_observer(&self) {
        self.inner.clear_action_result_observer();
    }
}
