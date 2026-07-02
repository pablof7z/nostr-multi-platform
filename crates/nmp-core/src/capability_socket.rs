//! Runtime capability callback socket shared by the actor and native hosts.
//!
//! The platform registers one native callback. Kernel modules issue typed
//! capability requests as JSON, this socket routes them to the native handler,
//! and the handler returns a typed envelope as JSON. Failures are represented
//! as data (D6), never as panics or NULL returns to the caller.
//!
//! **Rust-native path** (`set_native_handler`) — takes a `NativeCapabilityHandler`
//! closure for use by `nmp-uniffi`. `in_flight` + a `Condvar` gate the
//! registration so a setter waits for any in-flight invocation to drain
//! before returning, letting hosts release the previous closure's captured
//! state immediately after the setter returns.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// Rust-native capability handler — receives a `CapabilityRequest` JSON string
/// and returns a `CapabilityEnvelope` JSON string. Used by the UniFFI surface
/// (M14-C4).
///
/// The closure is called with the request JSON pre-copied (no Rust lock held
/// across the call).
pub type NativeCapabilityHandler = Arc<dyn Fn(String) -> String + Send + Sync + 'static>;

/// Mutable state for the capability-callback quiescence gate.
///
/// `in_flight > 0` only while Rust is actively invoking the native capability
/// callback copied from `handler`. Set/replace/unregister waits for this
/// counter to drain before returning so hosts can release callback state
/// immediately after the setter returns.
struct CapabilityCallbackGateInner {
    handler: Option<NativeCapabilityHandler>,
    in_flight: u32,
}

/// Quiescence-safe slot for the native capability callback registration.
///
/// After replacing or clearing the registration, the previous handler is
/// neither registered nor mid-invocation. Native bridges may drop the
/// previous closure's captured state after the setter returns.
pub struct CapabilityCallbackGate {
    inner: Mutex<CapabilityCallbackGateInner>,
    drained: Condvar,
}

impl CapabilityCallbackGate {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CapabilityCallbackGateInner {
                handler: None,
                in_flight: 0,
            }),
            drained: Condvar::new(),
        }
    }

    /// Replace or clear the Rust-native capability handler, then wait for all
    /// in-flight invocations to complete before returning.
    ///
    /// Used by the UniFFI surface (M14-C4) to register a `CapabilitySink`
    /// without a C function-pointer trampoline.
    ///
    /// Re-entrancy note: a native capability callback must not call the
    /// setter for the same slot from inside the callback, because the setter
    /// waits for that callback to finish.
    pub fn set_native_handler(&self, handler: Option<NativeCapabilityHandler>) {
        let mut guard = self.lock_inner();
        guard.handler = handler;
        drop(self.wait_drained(guard));
    }

    pub fn clear(&self) {
        let mut guard = self.lock_inner();
        guard.handler = None;
        drop(self.wait_drained(guard));
    }

    #[must_use]
    pub fn is_registered(&self) -> bool {
        self.lock_inner().handler.is_some()
    }

    fn begin_invocation(&self) -> Option<(NativeCapabilityHandler, CapabilityInvocation<'_>)> {
        let mut guard = self.lock_inner();
        let handler = Arc::clone(guard.handler.as_ref()?);
        guard.in_flight = guard.in_flight.saturating_add(1);
        Some((handler, CapabilityInvocation { gate: self }))
    }

    fn finish_invocation(&self) {
        let mut guard = self.lock_inner();
        guard.in_flight = guard.in_flight.saturating_sub(1);
        if guard.in_flight == 0 {
            self.drained.notify_all();
        }
    }

    fn lock_inner(&self) -> MutexGuard<'_, CapabilityCallbackGateInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wait_drained<'a>(
        &'a self,
        guard: MutexGuard<'a, CapabilityCallbackGateInner>,
    ) -> MutexGuard<'a, CapabilityCallbackGateInner> {
        self.drained
            .wait_while(guard, |inner| inner.in_flight > 0)
            .unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for CapabilityCallbackGate {
    fn default() -> Self {
        Self::new()
    }
}

struct CapabilityInvocation<'a> {
    gate: &'a CapabilityCallbackGate,
}

impl Drop for CapabilityInvocation<'_> {
    fn drop(&mut self) {
        self.gate.finish_invocation();
    }
}

pub type CapabilityCallbackSlot = Arc<CapabilityCallbackGate>;

#[must_use]
pub fn new_capability_callback_slot() -> CapabilityCallbackSlot {
    Arc::new(CapabilityCallbackGate::new())
}

/// Invoke the registered capability handler with `request_json` and return the
/// `CapabilityEnvelope` JSON. Pure data in, data out (D6): a missing handler
/// is reported as an error envelope.
///
/// The `in_flight` counter is incremented before releasing the gate lock, so
/// the handler is guaranteed live for the duration of the call.
pub fn dispatch_capability(slot: &CapabilityCallbackSlot, request_json: &str) -> String {
    let Some((handler, _invocation)) = slot.begin_invocation() else {
        return capability_error_envelope(request_json, "no-capability-handler");
    };
    // Copy the request BEFORE the foreign call — no Rust lock held here,
    // matching the UpdateSink quiescence contract.
    let request_owned = request_json.to_string();
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || handler(request_owned)));
    result.unwrap_or_else(|_| capability_error_envelope(request_json, "handler-panicked"))
}

/// Best-effort error `CapabilityEnvelope` (D6: failures are data). The
/// `namespace`/`correlation_id` are echoed from the request when parseable so
/// the issuing module can still correlate the failure.
pub fn capability_error_envelope(request_json: &str, reason: &str) -> String {
    let (namespace, correlation_id) = serde_json::from_str::<serde_json::Value>(request_json)
        .ok()
        .map(|v| {
            (
                v.get("namespace")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("correlation_id")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .unwrap_or_default();
    let result_json = format!(r#"{{"status":"error","os_status":-50,"reason":"{reason}"}}"#);
    serde_json::to_string(&crate::substrate::CapabilityEnvelope {
        namespace,
        correlation_id,
        result_json,
    })
    .unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
#[path = "capability_socket_quiescence_tests.rs"]
mod capability_socket_quiescence_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_handler_returns_error_envelope() {
        let slot = new_capability_callback_slot();
        let out = dispatch_capability(&slot, r#"{"namespace":"test","correlation_id":"c1"}"#);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["namespace"], "test");
        assert_eq!(v["correlation_id"], "c1");
        assert!(v["result_json"]
            .as_str()
            .unwrap()
            .contains("no-capability-handler"));
    }

    #[test]
    fn working_handler_response_is_returned() {
        let slot = new_capability_callback_slot();
        slot.set_native_handler(Some(Arc::new(|req: String| req)));
        let req = r#"{"namespace":"ks","correlation_id":"c3"}"#;
        let out = dispatch_capability(&slot, req);
        // The handler echoes the request JSON verbatim.
        assert_eq!(out, req);
    }

    #[test]
    fn error_envelope_echoes_namespace_and_correlation_id() {
        let req = r#"{"namespace":"myns","correlation_id":"abc"}"#;
        let out = capability_error_envelope(req, "test-reason");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["namespace"], "myns");
        assert_eq!(v["correlation_id"], "abc");
        assert!(v["result_json"].as_str().unwrap().contains("test-reason"));
    }

    #[test]
    fn error_envelope_degrades_on_unparseable_input() {
        let out = capability_error_envelope("not-json", "bad-input");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        // namespace and correlation_id default to empty string
        assert_eq!(v["namespace"], "");
        assert_eq!(v["correlation_id"], "");
        assert!(v["result_json"].as_str().unwrap().contains("bad-input"));
    }
}
