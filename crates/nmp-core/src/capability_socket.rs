//! Runtime capability callback socket shared by FFI and actor-owned effects.
//!
//! The platform registers one native callback. Kernel modules issue typed
//! capability requests as JSON, this socket routes them to the native handler,
//! and the handler returns a typed envelope as JSON. Failures are represented
//! as data (D6), never as panics or NULL returns to the caller.
//!
//! Two registration paths share the same quiescence gate:
//! * **C-ABI path** (`set_registration`) — takes a `CapabilityCallbackRegistration`
//!   (C function pointer + context) for use by `nmp-ffi`.
//! * **Rust-native path** (`set_native_handler`) — takes a `NativeCapabilityHandler`
//!   closure for use by `nmp-uniffi`. Uses the same `in_flight` + Condvar drain;
//!   setting one path clears the other (last-writer-wins).

use std::ffi::{c_char, c_void, CString};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// Native capability handler. Receives a `CapabilityRequest` JSON
/// (`*const c_char`, NUL-terminated, UTF-8) and returns a freshly heap-
/// allocated `CapabilityEnvelope` JSON string (`*mut c_char`) the caller must
/// release. A NULL return is converted to an error envelope.
pub type CapabilityCallback = extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;

#[derive(Clone, Copy)]
pub struct CapabilityCallbackRegistration {
    pub context: usize,
    pub callback: CapabilityCallback,
}

/// Rust-native capability handler — receives a `CapabilityRequest` JSON string
/// and returns a `CapabilityEnvelope` JSON string. Used by the UniFFI surface
/// (M14-C4) to avoid the C-ABI `extern "C"` trampoline.
///
/// The closure is called with the request JSON pre-copied (no Rust lock held
/// across the call), exactly mirroring the C-ABI quiescence contract.
pub type NativeCapabilityHandler = Arc<dyn Fn(String) -> String + Send + Sync + 'static>;

/// Discriminates which registration path is active.
enum CapabilityHandler {
    /// C-ABI path (nmp-ffi).
    CFfi(CapabilityCallbackRegistration),
    /// Rust-native path (nmp-uniffi).
    Native(NativeCapabilityHandler),
}

/// Mutable state for the capability-callback quiescence gate.
///
/// `in_flight > 0` only while Rust is actively invoking a native capability
/// callback copied from `handler`. Set/replace/unregister waits for this
/// counter to drain before returning so hosts can release callback contexts
/// immediately after the setter returns.
struct CapabilityCallbackGateInner {
    handler: Option<CapabilityHandler>,
    in_flight: u32,
}

/// Quiescence-safe slot for the native capability callback registration.
///
/// Mirrors the FFI update-callback contract: after replacing or clearing the
/// registration, the previous `(callback, context)` pair is neither registered
/// nor mid-invocation. Native bridges may free or release the previous context
/// after the setter returns.
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

    /// Replace or clear the C-ABI callback registration, then wait for all
    /// in-flight invocations to complete before returning.
    ///
    /// Clears any active Rust-native handler (last-writer-wins: a C-ABI
    /// registration replaces the UniFFI handler and vice versa).
    ///
    /// Re-entrancy matches the update callback: a native capability callback
    /// must not call the setter for the same slot from inside the callback,
    /// because the setter waits for that callback to finish.
    pub fn set_registration(&self, registration: Option<CapabilityCallbackRegistration>) {
        let mut guard = self.lock_inner();
        guard.handler = registration.map(CapabilityHandler::CFfi);
        drop(self.wait_drained(guard));
    }

    /// Replace or clear the Rust-native capability handler, then wait for all
    /// in-flight invocations to complete before returning.
    ///
    /// Clears any active C-ABI registration (last-writer-wins). Used by the
    /// UniFFI surface (M14-C4) to register a `CapabilitySink` without a C
    /// function-pointer trampoline.
    ///
    /// Same quiescence contract as [`Self::set_registration`]: after this
    /// returns, the previous handler is neither registered nor mid-invocation.
    pub fn set_native_handler(&self, handler: Option<NativeCapabilityHandler>) {
        let mut guard = self.lock_inner();
        guard.handler = handler.map(CapabilityHandler::Native);
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

    fn begin_invocation(&self) -> Option<(CapabilityHandleSnapshot, CapabilityInvocation<'_>)> {
        let mut guard = self.lock_inner();
        let snapshot = match guard.handler.as_ref()? {
            CapabilityHandler::CFfi(r) => CapabilityHandleSnapshot::CFfi(*r),
            CapabilityHandler::Native(f) => CapabilityHandleSnapshot::Native(Arc::clone(f)),
        };
        guard.in_flight = guard.in_flight.saturating_add(1);
        Some((snapshot, CapabilityInvocation { gate: self }))
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

/// Snapshot of the active handler, taken while holding the gate lock and before
/// `in_flight` is incremented. Returned by `begin_invocation` to decouple the
/// actual dispatch from the lock.
enum CapabilityHandleSnapshot {
    CFfi(CapabilityCallbackRegistration),
    Native(NativeCapabilityHandler),
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
/// or NULL native return is reported as an error envelope.
///
/// Dispatches to whichever path is active (C-ABI or Rust-native). The
/// `in_flight` counter is incremented before releasing the gate lock, so the
/// handler is guaranteed live for the duration of the call.
pub fn dispatch_capability(slot: &CapabilityCallbackSlot, request_json: &str) -> String {
    let Some((snapshot, _invocation)) = slot.begin_invocation() else {
        return capability_error_envelope(request_json, "no-capability-handler");
    };
    match snapshot {
        CapabilityHandleSnapshot::CFfi(registration) => {
            let Ok(request) = CString::new(request_json) else {
                return capability_error_envelope(request_json, "malformed-request");
            };
            let Some(raw) = crate::ffi_guard::guard_ffi_callback("capability handler", || {
                (registration.callback)(registration.context as *mut c_void, request.as_ptr())
            }) else {
                return capability_error_envelope(request_json, "handler-panicked");
            };
            if raw.is_null() {
                return capability_error_envelope(request_json, "handler-returned-null");
            }
            // SAFETY: a non-NULL return is contractually a CString allocated by the
            // native handler; we take ownership and free it on drop.
            let owned = unsafe { CString::from_raw(raw) };
            owned.to_string_lossy().into_owned()
        }
        CapabilityHandleSnapshot::Native(handler) => {
            // Copy the request BEFORE the foreign call — no Rust lock held here,
            // matching the C-ABI and UpdateSink quiescence contract.
            let request_owned = request_json.to_string();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                handler(request_owned)
            }));
            result.unwrap_or_else(|_| capability_error_envelope(request_json, "handler-panicked"))
        }
    }
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

    extern "C" fn echo_handler(_ctx: *mut c_void, req: *const c_char) -> *mut c_char {
        // Echo the request back as the response — simplest valid handler.
        let s = unsafe { std::ffi::CStr::from_ptr(req) }
            .to_string_lossy()
            .into_owned();
        CString::new(s).unwrap().into_raw()
    }

    extern "C" fn null_handler(_ctx: *mut c_void, _req: *const c_char) -> *mut c_char {
        std::ptr::null_mut()
    }

    fn install(slot: &CapabilityCallbackSlot, cb: CapabilityCallback) {
        slot.set_registration(Some(CapabilityCallbackRegistration {
            context: 0,
            callback: cb,
        }));
    }

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
    fn null_return_becomes_error_envelope() {
        let slot = new_capability_callback_slot();
        install(&slot, null_handler);
        let out = dispatch_capability(&slot, r#"{"namespace":"ns","correlation_id":"c2"}"#);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["result_json"]
            .as_str()
            .unwrap()
            .contains("handler-returned-null"));
    }

    #[test]
    fn working_handler_response_is_returned() {
        let slot = new_capability_callback_slot();
        install(&slot, echo_handler);
        let req = r#"{"namespace":"ks","correlation_id":"c3"}"#;
        let out = dispatch_capability(&slot, req);
        // echo_handler returns the request JSON verbatim.
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
