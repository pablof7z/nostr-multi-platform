//! Action-lane UniFFI methods — M14-C4.
//!
//! Mirrors `nmp-ffi/src/action.rs` for the `ack_action_stage` and
//! `register_action_result_observer` symbols.
//!
//! ## Quiescence note for `register_action_result_observer`
//!
//! The `ActionRegistry::deliver_result` implementation holds the
//! `Arc<Mutex>` lock ACROSS the observer call (mutual-exclusion quiescence)
//! rather than using the `Condvar` + `in_flight` pattern from
//! `UpdateListenerGate` / `CapabilityCallbackGate`. There is also no
//! `clear_result_observer` API on the registry.
//!
//! Per M14-C4 spec this is a **stop-and-report**: a proper drain gate for
//! the action-result observer is out of scope for this slice. The UniFFI
//! binding therefore:
//! * Supports registration and replacement (mutex exclusion makes replacement
//!   safe: `set_result_observer` waits for the mutex, which `deliver_result`
//!   holds across the callback, so the old observer has completed when the
//!   new one is installed).
//! * Does NOT expose a `clear` API (mirrors the C-ABI where null observer is a
//!   silent no-op).
//! * Does NOT include Barrier-style quiescence/teardown tests for this
//!   observer (those require the drain-gate pattern).
//!
//! ## Threading
//!
//! Both methods are non-blocking (D8):
//! * `ack_action_stage` sends `ActorCommand::ActionLedger(Ack(...))` down the
//!   actor channel.
//! * `register_action_result_observer` replaces the observer slot (mutex swap,
//!   no actor involvement).

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
    /// A second registration replaces the first. There is no clear API:
    /// passing `None` would be a no-op (mirrors the C-ABI null-observer
    /// behaviour). See the module-level quiescence note.
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
}
