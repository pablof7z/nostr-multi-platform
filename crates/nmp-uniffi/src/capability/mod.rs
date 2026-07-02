//! Capability, action-lane, and publish-control UniFFI surface.
//!
//! `nmp-uniffi` is the sole native binding surface for capability round-trips,
//! the action lane, and publish control (M14 complete; the legacy `nmp-ffi`
//! C-ABI crate has been deleted). Each sub-module adds a
//! `#[uniffi::export] impl NmpApp` block exposing typed methods.
//!
//! ## Module layout
//!
//! | Module       | UniFFI methods                                          |
//! |--------------|---------------------------------------------------------|
//! | `capability` | `set_capability_callback`, `dispatch_capability`        |
//! | `action`     | `ack_action_stage`, `register_action_result_observer`   |
//! | `publish`    | `retry_publish`, `cancel_action`                        |
//! | `lifecycle`  | `lifecycle_*`, `set_lifecycle_callback`, `is_alive`     |
//!
//! ## Callback interfaces
//!
//! ### `CapabilitySink` (request–response round-trip)
//!
//! The kernel issues a `CapabilityRequest` JSON, calls `on_capability_request`,
//! and expects a `CapabilityEnvelope` JSON back. `CapabilityCallbackGate`'s
//! Rust-native handler slot (`set_native_handler`) backs this with the same
//! `in_flight` + `Condvar` quiescence gate used across every kernel callback
//! seam. After `set_capability_callback` returns the previous sink is neither
//! registered nor mid-invocation.
//!
//! ### `ActionResultObserver` (push signal)
//!
//! The runtime's `ActionRegistry` now uses the same `in_flight` + `Condvar`
//! drain contract as update and capability callbacks. After replacing or
//! clearing the observer, the previous sink is neither registered nor
//! mid-invocation.
//!
//! ## Design notes
//!
//! * Each sub-module adds a `#[uniffi::export] impl NmpApp` block.
//! * Every method calls `self.inner.<method>()` on the underlying
//!   `nmp_native_runtime::NmpApp` — no logic is duplicated here.
//! * `CapabilitySink::on_capability_request` receives a pre-copied `String`
//!   (no Rust lock held across the foreign call) and returns a `String`.

pub mod action;
pub mod capability;
pub mod publish;

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
///   `clear_action_result_observer` from inside this method; the setter drains
///   in-flight callbacks before returning.
/// * Call `clear_action_result_observer` to unregister.
#[uniffi::export(callback_interface)]
pub trait ActionResultObserver: Send + Sync {
    fn on_action_result(&self, result_json: String);
}

#[cfg(test)]
pub(crate) mod action_tests;
#[cfg(test)]
pub(crate) mod tests;
