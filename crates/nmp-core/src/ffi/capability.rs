//! FFI capability-callback socket.
//!
//! Routes a kernel `CapabilityRequest` JSON to a registered native handler
//! and returns the resulting `CapabilityEnvelope` JSON. This is the seam the
//! Swift `KeychainCapability.handleJSON(_:)` plugs into for the keyring
//! capability (PD-019 / T96).
//!
//! Doctrine (`docs/product-spec/doctrine.md`):
//! * **D6** — nothing ever crosses this boundary as an exception. A missing
//!   handler, malformed request, or NULL native return all come back as a
//!   populated error `CapabilityEnvelope` (`result_json` describes it as
//!   data). A non-NULL `app`/`request` never yields a NULL return.
//! * **D7** — this socket only transports envelopes. It decides no policy:
//!   *which* capability, *what* operation, and *how* to react to the result
//!   are the issuing module's concern (see `substrate::KeyringIdentityWiring`).

use super::{app_ref, NmpApp};
use std::ffi::{c_char, c_void, CString};
use std::sync::{Arc, Mutex};

/// Native capability handler. Receives the kernel's `CapabilityRequest` JSON
/// (`*const c_char`, NUL-terminated, UTF-8) and returns a freshly heap-
/// allocated `CapabilityEnvelope` JSON string (`*mut c_char`) the kernel must
/// release via [`nmp_app_free_string`]. A NULL return is the sole exceptional
/// signal and is itself converted to an error envelope on the Rust side.
type CapabilityCallback = extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;

#[derive(Clone, Copy)]
pub(crate) struct CapabilityCallbackRegistration {
    context: usize,
    callback: CapabilityCallback,
}

/// Register the native capability handler. The kernel routes every
/// `CapabilityRequest` JSON through this seam (e.g. Swift's
/// `KeychainCapability.handleJSON(_:)`). Passing `None` unregisters; a
/// request received while unregistered yields an error envelope (D6), never
/// a crash.
#[no_mangle]
pub extern "C" fn nmp_app_set_capability_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<CapabilityCallback>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Ok(mut slot) = app.capability_callback.lock() else {
        return;
    };
    *slot = callback.map(|callback| CapabilityCallbackRegistration {
        context: context as usize,
        callback,
    });
}

/// Route a `CapabilityRequest` JSON to the registered native handler and
/// return the resulting `CapabilityEnvelope` JSON. The returned pointer is
/// heap-allocated by Rust and MUST be released via [`nmp_app_free_string`].
///
/// D6: never returns NULL for a non-NULL `app`/`request_json`; a missing
/// handler, malformed request, or a NULL handler return all come back as a
/// populated error `CapabilityEnvelope`. Errors are data, not exceptions.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_dispatch_capability(
    app: *mut NmpApp,
    request_json: *const c_char,
) -> *mut c_char {
    let request = super::c_string_argument(request_json).unwrap_or_default();
    let envelope = match app_ref(app) {
        Some(app) => dispatch_capability(&app.capability_callback, &request),
        None => capability_error_envelope(&request, "kernel-unavailable"),
    };
    // JSON never contains an interior NUL; the `c"{}"` literal fallback is
    // NUL-checked at compile time, so there is no runtime panic path (D6).
    CString::new(envelope).unwrap_or_else(|_| c"{}".to_owned()).into_raw()
}

/// Release a string previously returned by [`nmp_app_dispatch_capability`].
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        // SAFETY: caller guarantees ptr came from a `CString::into_raw`
        // returned by this module and is freed exactly once.
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

/// Invoke the registered native capability handler with `request_json` and
/// return the `CapabilityEnvelope` JSON. Pure data in, data out (D6): a
/// missing handler or NULL native return is reported as an error envelope.
/// Shared by the FFI entry point and the test harness's mock handler.
fn dispatch_capability(
    slot: &Arc<Mutex<Option<CapabilityCallbackRegistration>>>,
    request_json: &str,
) -> String {
    let registration = slot.lock().ok().and_then(|guard| *guard);
    let Some(registration) = registration else {
        return capability_error_envelope(request_json, "no-capability-handler");
    };
    let Ok(request) = CString::new(request_json) else {
        return capability_error_envelope(request_json, "malformed-request");
    };
    let raw = (registration.callback)(registration.context as *mut c_void, request.as_ptr());
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
fn capability_error_envelope(request_json: &str, reason: &str) -> String {
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
    //! Round-trips the kernel-side `KeyringCapability` contract through this
    //! FFI capability socket against a mock native handler that speaks the
    //! exact JSON the iOS `KeychainCapability` speaks. Covers
    //! store -> retrieve -> delete -> not_found and the D6 error paths.

    use super::*;
    use crate::substrate::{
        CapabilityEnvelope, CapabilityModule, KeyringCapability, KeyringIdentityWiring,
        KeyringRequest, KeyringResult, KeyringStatus,
    };
    use std::collections::HashMap;
    use std::ffi::CStr;

    // In-memory secret store standing in for the iOS Keychain. The mock
    // handler is a plain `extern "C"` fn (FFI shape) and cannot capture
    // state, so the store is a `static`; tests share one via `SERIAL`.
    static STORE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Mock native handler — decodes the kernel `CapabilityRequest`, runs the
    /// keyring op against `STORE`, returns a `CapabilityEnvelope` JSON. Never
    /// panics across the boundary; every failure is data (D6). Mirrors
    /// `KeychainCapability.handleJSON(_:)`.
    extern "C" fn mock_handler(_ctx: *mut c_void, request_json: *const c_char) -> *mut c_char {
        let request = unsafe { CStr::from_ptr(request_json) }
            .to_str()
            .unwrap_or("");
        let parsed: serde_json::Value = serde_json::from_str(request).unwrap_or_default();
        let correlation_id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let payload = parsed
            .get("payload_json")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let result = match serde_json::from_str::<KeyringRequest>(payload) {
            Ok(KeyringRequest::Store { account_id, secret }) => {
                STORE
                    .lock()
                    .unwrap()
                    .get_or_insert_with(HashMap::new)
                    .insert(account_id, secret);
                KeyringResult::ok(None)
            }
            Ok(KeyringRequest::Retrieve { account_id }) => {
                match STORE
                    .lock()
                    .unwrap()
                    .get_or_insert_with(HashMap::new)
                    .get(&account_id)
                {
                    Some(secret) => KeyringResult::ok(Some(secret.clone())),
                    None => KeyringResult::not_found(),
                }
            }
            Ok(KeyringRequest::Delete { account_id }) => {
                STORE
                    .lock()
                    .unwrap()
                    .get_or_insert_with(HashMap::new)
                    .remove(&account_id);
                KeyringResult::ok(None)
            }
            Err(_) => KeyringResult::error(-50),
        };

        let envelope = CapabilityEnvelope {
            namespace: KeyringCapability::NAMESPACE.to_string(),
            correlation_id,
            result_json: serde_json::to_string(&result).unwrap(),
        };
        CString::new(serde_json::to_string(&envelope).unwrap())
            .unwrap()
            .into_raw()
    }

    fn registered_slot() -> Arc<Mutex<Option<CapabilityCallbackRegistration>>> {
        Arc::new(Mutex::new(Some(CapabilityCallbackRegistration {
            context: 0,
            callback: mock_handler,
        })))
    }

    fn run(
        slot: &Arc<Mutex<Option<CapabilityCallbackRegistration>>>,
        req: &crate::substrate::CapabilityRequest,
    ) -> KeyringResult {
        let json = serde_json::to_string(req).unwrap();
        let envelope: CapabilityEnvelope =
            serde_json::from_str(&dispatch_capability(slot, &json)).unwrap();
        assert_eq!(envelope.correlation_id, req.correlation_id);
        assert_eq!(envelope.namespace, KeyringCapability::NAMESPACE);
        KeyringIdentityWiring::decode_result(&envelope)
    }

    #[test]
    fn store_retrieve_delete_round_trip() {
        let _g = SERIAL.lock().unwrap();
        *STORE.lock().unwrap() = Some(HashMap::new());
        let slot = registered_slot();

        assert_eq!(
            run(
                &slot,
                &KeyringIdentityWiring::persist_secret("c1", "acct-1", "nsec1secret")
            ),
            KeyringResult::ok(None)
        );

        let recalled = run(&slot, &KeyringIdentityWiring::recall_secret("c2", "acct-1"));
        assert_eq!(recalled.status, KeyringStatus::Ok);
        assert_eq!(recalled.secret.as_deref(), Some("nsec1secret"));

        assert_eq!(
            run(&slot, &KeyringIdentityWiring::forget_secret("c3", "acct-1")),
            KeyringResult::ok(None)
        );

        assert_eq!(
            run(&slot, &KeyringIdentityWiring::recall_secret("c4", "acct-1")).status,
            KeyringStatus::NotFound
        );
    }

    #[test]
    fn missing_handler_yields_error_envelope_not_panic() {
        let _g = SERIAL.lock().unwrap();
        let empty: Arc<Mutex<Option<CapabilityCallbackRegistration>>> = Arc::new(Mutex::new(None));
        let req = KeyringIdentityWiring::persist_secret("c9", "acct-x", "nsec1");
        let json = serde_json::to_string(&req).unwrap();

        let envelope: CapabilityEnvelope =
            serde_json::from_str(&dispatch_capability(&empty, &json)).unwrap();
        assert_eq!(envelope.correlation_id, "c9");
        assert_eq!(envelope.namespace, KeyringCapability::NAMESPACE);
        assert_eq!(
            KeyringIdentityWiring::decode_result(&envelope).status,
            KeyringStatus::Error
        );
    }

    #[test]
    fn handler_returning_null_is_reported_as_data() {
        extern "C" fn null_handler(_c: *mut c_void, _r: *const c_char) -> *mut c_char {
            std::ptr::null_mut()
        }
        let slot = Arc::new(Mutex::new(Some(CapabilityCallbackRegistration {
            context: 0,
            callback: null_handler,
        })));
        let req = KeyringIdentityWiring::recall_secret("c0", "acct-1");
        let json = serde_json::to_string(&req).unwrap();
        let envelope: CapabilityEnvelope =
            serde_json::from_str(&dispatch_capability(&slot, &json)).unwrap();
        assert_eq!(envelope.correlation_id, "c0");
        assert_eq!(
            KeyringIdentityWiring::decode_result(&envelope).status,
            KeyringStatus::Error
        );
    }
}
