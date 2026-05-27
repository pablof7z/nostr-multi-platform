//! Runtime capability callback socket shared by FFI and actor-owned effects.
//!
//! The platform registers one native callback. Kernel modules issue typed
//! capability requests as JSON, this socket routes them to the native handler,
//! and the handler returns a typed envelope as JSON. Failures are represented
//! as data (D6), never as panics or NULL returns to the caller.

use std::ffi::{c_char, c_void, CString};
use std::sync::{Arc, Mutex};

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

pub type CapabilityCallbackSlot = Arc<Mutex<Option<CapabilityCallbackRegistration>>>;

#[must_use]
pub fn new_capability_callback_slot() -> CapabilityCallbackSlot {
    Arc::new(Mutex::new(None))
}

/// Invoke the registered native capability handler with `request_json` and
/// return the `CapabilityEnvelope` JSON. Pure data in, data out (D6): a
/// missing handler or NULL native return is reported as an error envelope.
pub fn dispatch_capability(slot: &CapabilityCallbackSlot, request_json: &str) -> String {
    let registration = slot.lock().ok().and_then(|guard| *guard);
    let Some(registration) = registration else {
        return capability_error_envelope(request_json, "no-capability-handler");
    };
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
        *slot.lock().unwrap() = Some(CapabilityCallbackRegistration {
            context: 0,
            callback: cb,
        });
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
