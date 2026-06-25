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
//! Local-reference hygiene: each `call()` body runs inside `with_local_frame`
//! so the JNI local-reference table is reclaimed on every dispatch. Without
//! this, dispatches arriving on an already-attached thread (where the
//! `AttachGuard` never detaches) would leak ~3 local refs each — the new
//! String argument, the returned object, and the `JString` wrapper — and
//! eventually overflow the local-ref table.
//!
//! Teardown / UAF safety
//! ---------------------
//! The synchronous handler's `GlobalRef` is dropped by `close_updates_locked`
//! after `nmp_app_set_capability_callback(None)` returns. The callback setter
//! is quiescent: it blocks until any in-flight capability trampoline has
//! returned, matching the update-callback contract. `call_sync_handler` still
//! holds the `capability_handler` lock for the entire duration of
//! `handler.call()` so explicit handler replacement/clearing through
//! `nativeSetCapabilityHandler` also serializes against a live JNI upcall.
//!
//! Doctrine
//! --------
//! * **D6** — every failure (detached VM, JNI error, null handler return) is
//!   reported as a `capability_error_envelope`, never a panic or NULL.
//! * **D7** — this module transports envelopes. It decides no policy.

use std::sync::Mutex;

use jni::JavaVM;
use jni::objects::GlobalRef;
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

        // Run the whole upcall inside a local-reference frame so every JNI
        // local ref allocated below (the request String, the returned object,
        // and the JString wrapper) is reclaimed when the frame pops — required
        // because dispatches on an already-attached thread never detach
        // (no AttachGuard cleanup), which would otherwise leak ~3 locals per
        // dispatch and eventually overflow the local-ref table.
        let outcome = env.with_local_frame(8, |env| -> Result<String, jni::errors::Error> {
            // `new_string` builds a Java String via JNI NewStringUTF, which
            // uses *modified UTF-8*. Safe today because keyring payloads are
            // ASCII (base64 ciphertext + opaque account-id) — but supplementary
            // -plane (4-byte) code points would be corrupted. Any reuse of this
            // handler for richer (non-ASCII) payloads must revisit the encoding.
            let j_request = env.new_string(request_json)?;

            // Call `handler.handle(requestJson: String): String`.
            let val = env.call_method(
                &self.handler,
                "handle",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[jni::objects::JValueGen::Object(j_request.as_ref())],
            )?;

            let obj = val.l()?;
            if obj.is_null() {
                // Empty-string sentinel → "handler-returned-null" below. A real
                // CapabilityEnvelope is never empty (always at least `{}`), so
                // an empty result unambiguously means the handler returned null.
                return Ok(String::new());
            }
            // `get_string` reads via JNI GetStringUTFChars — also modified
            // UTF-8; the same ASCII-only caveat as `new_string` above applies
            // to the handler's returned envelope.
            // SAFETY: `obj` is the non-null String returned by `handle`; the
            // JString wrapper borrows it for the duration of `get_string`.
            let j_result = unsafe { jni::objects::JString::from_raw(obj.as_raw()) };
            let s = env.get_string(&j_result)?;
            Ok(s.into())
        });

        match outcome {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => capability_error_envelope(request_json, "handler-returned-null"),
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
///
/// LOAD-BEARING: the `slot` lock is held for the ENTIRE `h.call()` duration
/// (the `guard` outlives the `map` closure). Teardown is fenced by the
/// capability-callback quiescence gate; this lock additionally serializes
/// explicit handler replacement/clearing through `nativeSetCapabilityHandler`
/// against an active JNI upcall. Do not narrow this lock scope.
pub(crate) fn call_sync_handler(
    slot: &CapabilityHandlerSlot,
    request_json: &str,
) -> Option<String> {
    let guard = slot.lock().ok()?;
    guard.as_ref().map(|h| h.call(request_json))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that `call_sync_handler` with no registered handler returns `None`.
    #[test]
    fn no_handler_returns_none() {
        let slot: CapabilityHandlerSlot = Mutex::new(None);
        assert!(call_sync_handler(&slot, r#"{"namespace":"nmp.keyring.capability"}"#).is_none());
    }
}
