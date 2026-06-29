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
//! | `lifecycle`  | `lifecycle_*`, `set_lifecycle_callback`, `is_alive`     | `nmp-ffi/src/lifecycle.rs`     |
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
//! The runtime's `ActionRegistry` now uses the same `in_flight` + `Condvar`
//! drain contract as update and capability callbacks. After replacing or
//! clearing the observer, the previous sink is neither registered nor
//! mid-invocation.
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
