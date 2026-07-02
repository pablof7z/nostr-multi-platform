//! Capability-callback UniFFI methods.
//!
//! `set_capability_callback` and `dispatch_capability`. Both methods call
//! the underlying `nmp_native_runtime::NmpApp` / `CapabilityCallbackGate`
//! primitives directly.
//!
//! ## Quiescence
//!
//! `set_capability_callback` delegates to
//! `CapabilityCallbackGate::set_native_handler`, which uses the same
//! `in_flight` + `Condvar` drain contract as `set_update_listener`. After
//! this call returns the previous sink is neither registered nor
//! mid-invocation.
//!
//! The `on_capability_request` string is a pre-copied `String` value —
//! no Rust lock is held across the foreign call, matching the C-ABI contract.

use super::CapabilitySink;
use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Register (or clear) the capability-request handler.
    ///
    /// After this returns, the previous sink is guaranteed to be neither
    /// registered nor mid-invocation (same quiescence contract as
    /// `set_update_sink` — `CapabilityCallbackGate` uses `in_flight` + Condvar).
    ///
    /// Pass `None` to clear. Re-entrancy is forbidden: calling this from inside
    /// `on_capability_request` deadlocks the quiescence gate.
    ///
    /// # Mapping
    ///
    /// Registers a `NativeCapabilityHandler` (Rust closure) via
    /// `CapabilityCallbackGate::set_native_handler`. This is the Rust-native
    /// counterpart to the C-ABI `CapabilityCallback` fn-ptr path; both share
    /// the same quiescence gate.
    pub fn set_capability_callback(&self, sink: Option<Box<dyn CapabilitySink>>) {
        nmp_uniffi_support::set_capability_callback(&self.inner, sink, |sink, request_json| {
            sink.on_capability_request(request_json)
        });
    }

    /// Route a `CapabilityRequest` JSON to the registered handler and return
    /// the `CapabilityEnvelope` JSON.
    ///
    /// D6: never throws. A missing handler, malformed request, or panicking
    /// sink all come back as a populated error `CapabilityEnvelope`. Errors are
    /// data, not exceptions.
    ///
    /// This is the shell→Rust response half of the request–response round-trip:
    /// the shell calls `set_capability_callback` to register the request
    /// handler, and after processing a request it calls this method to deliver
    /// the response back to the kernel.
    pub fn dispatch_capability_json(&self, request_json: String) -> String {
        nmp_uniffi_support::dispatch_capability_json(&self.inner, &request_json)
    }
}
