//! Capability-callback UniFFI methods — M14-C4.
//!
//! Mirrors `nmp-ffi/src/capability.rs` for the `set_capability_callback` and
//! `dispatch_capability` symbols. Both methods call the SAME underlying
//! `nmp_native_runtime::NmpApp` / `CapabilityCallbackGate` primitives the
//! C-ABI wrapper calls.
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

use std::sync::Arc;

use nmp_core::__ffi_internal::{dispatch_capability, NativeCapabilityHandler};

use crate::NmpApp;
use super::CapabilitySink;

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
        let handler: Option<NativeCapabilityHandler> = sink.map(|s| {
            // Wrap in Arc so the closure is `Sync` (NativeCapabilityHandler requires
            // `Send + Sync`).
            let s: Arc<dyn CapabilitySink> = Arc::from(s);
            Arc::new(move |request_json: String| -> String {
                // `request_json` is already a pre-copied String passed in by
                // dispatch_capability — no Rust lock is held at this point.
                // Clone here so both the closure and the error path below can
                // use the value.
                let req_for_call = request_json.clone();
                // Panic containment: a Swift/Kotlin throw must not unwind into
                // the dispatch thread (D6).
                let s = Arc::clone(&s);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    s.on_capability_request(req_for_call)
                }));
                result.unwrap_or_else(|_| {
                    // D6: panics become error envelopes, never crashes.
                    nmp_core::__ffi_internal::capability_error_envelope(
                        &request_json,
                        "sink-panicked",
                    )
                })
            }) as NativeCapabilityHandler
        });
        self.inner.capability_callback_slot().set_native_handler(handler);
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
        let slot = self.inner.capability_callback_slot();
        dispatch_capability(&slot, &request_json)
    }
}
