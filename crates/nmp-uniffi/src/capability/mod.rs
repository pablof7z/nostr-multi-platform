//! Capability, action-lane, and publish-control UniFFI surface — M14-C4.
//!
//! Migrates the C-ABI symbols from `nmp-ffi/src/{capability,action,publish}.rs`
//! to typed `#[uniffi::export] impl NmpApp` methods. This is **additive** — the
//! C-ABI symbols are NOT deleted here (transitional until M14-D).
//!
//! ## Module layout
//!
//! | Module       | UniFFI methods                                          | C-ABI counterpart              |
//! |--------------|---------------------------------------------------------|--------------------------------|
//! | `capability` | `set_capability_callback`, `dispatch_capability`        | `nmp-ffi/src/capability.rs`    |
//! | `action`     | `ack_action_stage`, `register_action_result_observer`   | `nmp-ffi/src/action.rs`        |
//! | `publish`    | `retry_publish`, `cancel_action`                        | `nmp-ffi/src/publish.rs`       |
//!
//! ## Callback interfaces
//!
//! ### `CapabilitySink` (request–response round-trip)
//!
//! The kernel issues a `CapabilityRequest` JSON, calls `on_capability_request`,
//! and expects a `CapabilityEnvelope` JSON back. Maps to the C-ABI's
//! `CapabilityCallback fn ptr` path but uses `CapabilityCallbackGate`'s
//! Rust-native handler slot (`set_native_handler`) so the same `in_flight` +
//! `Condvar` quiescence gate that protects the C path also protects the UniFFI
//! path. After `set_capability_callback` returns the previous sink is neither
//! registered nor mid-invocation.
//!
//! ### `ActionResultObserver` (push signal)
//!
//! `ActionRegistry` now holds the result observer behind a `ResultObserverGate`
//! (the `Condvar` + `in_flight` drain shared with the capability socket and
//! update listener), so `register_action_result_observer` drains the previous
//! observer before returning and `clear_action_result_observer` is the teardown
//! counterpart. Full Barrier-style quiescence/teardown tests now cover this
//! observer (M14-C-tail / #2429).
//!
//! ## Design notes
//!
//! * Each sub-module adds a `#[uniffi::export] impl NmpApp` block.
//! * Every method calls `self.inner.<method>()` — the same underlying
//!   `nmp_native_runtime::NmpApp` method the C-ABI wrapper calls.
//! * `CapabilitySink::on_capability_request` receives a pre-copied `String`
//!   (no Rust lock held across the foreign call) and returns a `String`.

pub mod action;
pub mod capability;
pub mod publish;

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod action_observer_tests;

// ── Callback interfaces ───────────────────────────────────────────────────────

/// Rust→shell capability round-trip: the kernel calls this to route a
/// `CapabilityRequest` JSON to the platform (e.g. iOS Keychain) and expects a
/// `CapabilityEnvelope` JSON back.
///
/// # Contract
///
/// * `request_json` is a pre-copied JSON string — no Rust lock is held during
///   the call. The implementation may block; it MUST NOT call
///   `set_capability_callback` for the same app from inside this method
///   (reentrancy deadlocks the quiescence gate).
/// * The returned string must be a valid `CapabilityEnvelope` JSON
///   (`{"namespace":…,"correlation_id":…,"result_json":…}`). D6: a panic or
///   invalid return is caught and converted to an error envelope.
#[uniffi::export(callback_interface)]
pub trait CapabilitySink: Send + Sync {
    fn on_capability_request(&self, request_json: String) -> String;
}

/// Rust→shell push observer: fired after a dispatched action is accepted and
/// enqueued for execution.
///
/// # Contract
///
/// * `result_json` is a JSON string `{"correlation_id":"…","result_json":…}`.
/// * Implementations MUST NOT call `register_action_result_observer` or
///   `clear_action_result_observer` from inside this method: the drain gate
///   waits for this delivery to finish, so re-entry would deadlock.
/// * Unregister via `clear_action_result_observer`; both register and clear
///   drain any in-flight delivery before returning (M14-C-tail / #2429).
#[uniffi::export(callback_interface)]
pub trait ActionResultObserver: Send + Sync {
    fn on_action_result(&self, result_json: String);
}
