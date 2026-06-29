//! Action-lane UniFFI methods — M14-C4.
//!
//! Mirrors `nmp-ffi/src/action.rs` for the `ack_action_stage` and
//! `register_action_result_observer` symbols.
//!
//! ## Quiescence note for `register_action_result_observer`
//!
//! `ActionRegistry` uses the same `in_flight` + `Condvar` drain pattern as
//! `UpdateListenerGate` / `CapabilityCallbackGate`. Replacement and clear wait
//! for any in-flight callback before returning.
//!
//! ## Threading
//!
//! Both methods are non-blocking (D8):
//! * `ack_action_stage` sends `ActorCommand::ActionLedger(Ack(...))` down the
//!   actor channel.
//! * `register_action_result_observer` replaces the observer slot (mutex swap,
//!   no actor involvement).

use super::ActionResultObserver;
use crate::NmpApp;

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
        self.inner
            .send_cmd(nmp_core::actor::ActorCommand::ActionLedger(
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
    /// A second registration replaces the first and drains the previous sink
    /// before returning. Re-entrancy is forbidden: calling this or
    /// `clear_action_result_observer` from inside `on_action_result` deadlocks
    /// the quiescence gate.
    pub fn register_action_result_observer(&self, observer: Box<dyn ActionResultObserver>) {
        nmp_uniffi_support::register_action_result_observer(
            &self.inner,
            observer,
            |observer, json| {
                observer.on_action_result(json);
            },
        );
    }

    /// Clear the action-result observer and wait for any in-flight callback to
    /// finish before returning.
    pub fn clear_action_result_observer(&self) {
        nmp_uniffi_support::clear_action_result_observer(&self.inner);
    }
}
