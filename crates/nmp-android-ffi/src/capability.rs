//! Synchronous capability handler — namespace router for non-`external_signer`
//! capability namespaces (Android Keystore keyring, etc.).
//!
//! Architecture
//! ------------
//! The kernel capability socket dispatches ALL `CapabilityRequest` payloads to
//! the single trampoline registered in `nmp_app_set_capability_callback`. The
//! trampoline in `external_signer.rs` previously handled `external_signer`
//! namespaces via an async MPSC channel and error-enveloped everything else.
//!
//! This module introduces a **synchronous JNI upcall path** for namespaces that
//! do NOT need an async round-trip (e.g. Android Keystore AES-GCM operations
//! which complete inline). The Kotlin side registers one handler object via
//! `nativeSetCapabilityHandler`; the trampoline calls into it on whichever
//! Rust/JNI thread the capability dispatch arrives on.
//!
//! Thread safety
//! -------------
//! Capability requests may arrive on the kernel's actor thread or a dedicated
//! capability-worker thread. The JNI `call_method` is safe from any thread
//! provided the `JavaVM` is available to `attach_current_thread`. We hold the
//! `JavaVM` in the `SyncCapabilityHandler` and use `attach_current_thread` for
//! each upcall rather than caching the `JNIEnv` (which is thread-local).
//!
//! Teardown
//! --------
//! `clear` drops the `GlobalRef` and NULLs the `JavaVM`. After
//! `nmp_app_set_capability_callback(None)` quiesces any in-flight dispatch (the
//! FFI update-callback gate ensures this), `clear` is safe to call from
//! `close_updates_locked`.
//!
//! Doctrine
//! --------
//! * **D6** — every failure (detached VM, JNI error, null handler return) is
//!   reported as a `capability_error_envelope`, never a panic or NULL.
//! * **D7** — this module transports envelopes. It decides no policy.

use std::sync::Mutex;

use jni::objects::GlobalRef;
use jni::JavaVM;
use nmp_core::__ffi_internal::capability_error_envelope;

/// A Kotlin object implementing `fun handle(requestJson: String): String`.
/// Registered by `nativeSetCapabilityHandler`; cleared in `close_updates_locked`.
pub(crate) struct SyncCapabilityHandler {
    vm: JavaVM,
    handler: GlobalRef,
}

// SAFETY: `JavaVM` is `Send + Sync`. `GlobalRef` is safe to send across threads
// because the JVM tracks it; we only use it under `attach_current_thread`.
unsafe impl Send for SyncCapabilityHandler {}
unsafe impl Sync for SyncCapabilityHandler {}

impl SyncCapabilityHandler {
    pub(crate) fn new(vm: JavaVM, handler: GlobalRef) -> Self {
        Self { vm, handler }
    }

    /// Invoke the Kotlin handler synchronously. Returns the raw
    /// `CapabilityEnvelope` JSON string, or an error envelope on JNI failure.
    pub(crate) fn call(&self, request_json: &str) -> String {
        let mut env = match self.vm.attach_current_thread() {
            Ok(env) => env,
            Err(e) => {
                return capability_error_envelope(request_json, &format!("jni-attach-failed:{e}"));
            }
        };

        // Build the Java String argument.
        let j_request = match env.new_string(request_json) {
            Ok(s) => s,
            Err(e) => {
                return capability_error_envelope(
                    request_json,
                    &format!("jni-new-string-failed:{e}"),
                );
            }
        };

        // Call `handler.handle(requestJson: String): String`.
        let result = env.call_method(
            &self.handler,
            "handle",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[jni::objects::JValueGen::Object(j_request.as_ref())],
        );

        match result {
            Ok(val) => {
                // Extract the returned Java String.
                match val.l() {
                    Ok(obj) if !obj.is_null() => {
                        match env.get_string(unsafe { &jni::objects::JString::from_raw(obj.as_raw()) }) {
                            Ok(s) => s.into(),
                            Err(e) => capability_error_envelope(
                                request_json,
                                &format!("jni-get-string-failed:{e}"),
                            ),
                        }
                    }
                    _ => capability_error_envelope(request_json, "handler-returned-null"),
                }
            }
            Err(e) => {
                // Clear any pending JNI exception so the thread doesn't stay in
                // a poisoned state (D6: error is data, not an exception).
                let _ = env.exception_clear();
                capability_error_envelope(request_json, &format!("jni-call-failed:{e}"))
            }
        }
    }
}

/// Session-owned slot for the synchronous capability handler.
/// `Option<SyncCapabilityHandler>` behind a `Mutex` so registration and
/// teardown are independent of the session lock.
pub(crate) type CapabilityHandlerSlot = Mutex<Option<SyncCapabilityHandler>>;

/// Invoke the registered synchronous capability handler, if any.
/// Returns `None` when no handler is registered (caller should error-envelope).
pub(crate) fn call_sync_handler(slot: &CapabilityHandlerSlot, request_json: &str) -> Option<String> {
    let guard = slot.lock().ok()?;
    guard.as_ref().map(|h| h.call(request_json))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Verifies that `call_sync_handler` with no registered handler returns `None`.
    #[test]
    fn no_handler_returns_none() {
        let slot: CapabilityHandlerSlot = Mutex::new(None);
        assert!(call_sync_handler(&slot, r#"{"namespace":"nmp.keyring.capability"}"#).is_none());
    }
}
