//! JNI push listener for NIP-55 external-signer requests (issue #1284 — D8
//! no-polling).
//!
//! Direct twin of [`crate::update_listener::UpdatePushListener`]: it clones the
//! same GlobalRef-handler + JavaVM machinery so the kernel can push each
//! `ExternalSignerRequest` JSON straight to a registered Kotlin listener,
//! replacing the former 250 ms-timed `nativeNextSignerRequest` drain.

use std::sync::{Arc, Mutex};

use jni::objects::{GlobalRef, JClass, JObject};
use jni::sys::jlong;
use jni::{JNIEnv, JavaVM};

use crate::session::session_arc;

/// JNI push listener for NIP-55 external-signer request JSON.
///
/// Registered by `nativeSetSignerRequestListener`; invoked from the
/// `on_capability_request` trampoline (`external_signer.rs`) on whichever Rust
/// thread dispatches the `external_signer` capability; cleared in
/// `Session::close_updates_locked` AFTER `nmp_app_set_capability_callback(…,
/// None)` unregisters the trampoline.
///
/// UAF safety: the load-bearing guard is the capability-socket unregister in
/// `close_updates_locked`, which blocks until in-flight trampolines drain. This
/// listener follows the *update-callback* shape: the trampoline snapshots an
/// `Arc` clone of the listener under the slot lock and drops the lock before
/// the upcall, so replacement/clearing only ever races a cheap `Arc::clone`,
/// never the JNI `push` itself.
pub(crate) struct SignerRequestPushListener {
    vm: JavaVM,
    handler: GlobalRef,
}

// SAFETY: `JavaVM` is `Send + Sync`. `GlobalRef` is safe to send across threads
// because the JVM tracks it; we only dereference it under `attach_current_thread`.
unsafe impl Send for SignerRequestPushListener {}
unsafe impl Sync for SignerRequestPushListener {}

impl SignerRequestPushListener {
    pub(crate) fn new(vm: JavaVM, handler: GlobalRef) -> Self {
        Self { vm, handler }
    }

    /// Invoke `listener.onSignerRequest(requestJson: String)` on the Kotlin
    /// listener.
    ///
    /// Runs inside `with_local_frame` so the JNI local-reference table is
    /// reclaimed on every push (dispatches on an already-attached thread never
    /// detach, which would otherwise leak the `String` local each call). D6:
    /// every failure (detached VM, JNI error/exception) is swallowed — this
    /// callback never panics across the JNI seam.
    pub(crate) fn push(&self, request_json: &str) {
        let mut env = match self.vm.attach_current_thread() {
            Ok(env) => env,
            Err(_) => return,
        };
        let _ = env.with_local_frame(8, |env| -> Result<(), jni::errors::Error> {
            let arg = env.new_string(request_json)?;
            env.call_method(
                &self.handler,
                "onSignerRequest",
                "(Ljava/lang/String;)V",
                &[jni::objects::JValueGen::Object(arg.as_ref())],
            )?;
            Ok(())
        });
        // Clear any pending JNI exception so the thread isn't left poisoned (D6).
        let _ = env.exception_clear();
    }
}

/// Session-owned slot for the JNI push signer-request listener.
///
/// Wrapped in `Arc` so `on_capability_request` can snapshot the listener
/// reference, drop the lock, and then invoke `push` without holding the mutex
/// across the JNI boundary (deadlock prevention — mirrors the update path).
pub(crate) type SignerRequestListenerSlot = Mutex<Option<Arc<SignerRequestPushListener>>>;

/// NIP-55 signer-request listener accessors on [`crate::session::Session`].
///
/// Lives here (not in `session.rs`) to keep the core session module under the
/// 500 LOC hard cap while keeping every signer-request-listener concern in one
/// place. All methods operate on the `pub(crate)` listener/capture slots of
/// `Session`.
impl crate::session::Session {
    /// Register the JNI push listener for NIP-55 signer requests (issue #1284).
    /// Replaces an existing listener if one is already set. Cleared on teardown
    /// by `Session::close_updates_locked`.
    pub(crate) fn set_signer_request_listener(&self, listener: SignerRequestPushListener) {
        if let Ok(mut slot) = self.signer_request_listener.lock() {
            *slot = Some(Arc::new(listener));
        }
    }

    /// Drop the NIP-55 signer-request push listener (deregister). Safe to call
    /// when none is set.
    pub(crate) fn clear_signer_request_listener(&self) {
        if let Ok(mut slot) = self.signer_request_listener.lock() {
            slot.take();
        }
    }

    /// Push one `ExternalSignerRequest` JSON to the registered Kotlin listener,
    /// if any. Returns `true` when a listener consumed the request.
    ///
    /// Lock ordering: snapshot the `Arc` clone under the slot lock, drop the
    /// lock BEFORE the JNI upcall (mirrors `on_update`) so Kotlin re-entering a
    /// Rust JNI entry-point from inside `onSignerRequest` cannot deadlock.
    pub(crate) fn push_signer_request(&self, request_json: &str) -> bool {
        let listener_snapshot: Option<Arc<SignerRequestPushListener>> = self
            .signer_request_listener
            .lock()
            .ok()
            .and_then(|g| g.clone());
        if let Some(listener) = listener_snapshot {
            listener.push(request_json);
            return true;
        }
        // Test-only path: with no JVM-backed listener, route to the capture sink
        // (when armed) so the trampoline tests can assert the pushed payload.
        #[cfg(test)]
        {
            if let Ok(mut guard) = self.signer_request_capture.lock() {
                if let Some(sink) = guard.as_mut() {
                    sink.push(request_json.to_string());
                    return true;
                }
            }
        }
        false
    }

    /// Test-only: arm the signer-request capture sink so `push_signer_request`
    /// records pushed payloads (no JVM available in unit tests).
    #[cfg(test)]
    pub(crate) fn arm_signer_request_capture(&self) {
        if let Ok(mut guard) = self.signer_request_capture.lock() {
            *guard = Some(Vec::new());
        }
    }

    /// Test-only: drain the captured signer-request payloads.
    #[cfg(test)]
    pub(crate) fn captured_signer_requests(&self) -> Vec<String> {
        self.signer_request_capture
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default()
    }
}

/// Register (or clear) the JNI push listener for NIP-55 external-signer
/// requests (issue #1284 — D8 no-polling; replaces the deleted
/// `nativeNextSignerRequest` blocking drain).
///
/// `listener` must implement `fun onSignerRequest(requestJson: String)`. Each
/// request is pushed from whichever Rust thread dispatches the `external_signer`
/// capability (a background thread), so Kotlin must treat `onSignerRequest` as a
/// background callback and marshal to the main thread itself (the NIP-55 Intent
/// dispatch requires the main thread). Pass `null` to deregister.
///
/// D6: a null/dead handle, or any JNI failure obtaining the `JavaVM` / global
/// ref, is a silent no-op — never panics across the seam. The listener
/// `GlobalRef` is dropped on teardown (`nativeClose`/`nativeFree`) after the
/// capability trampoline is unregistered.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSetSignerRequestListener(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    listener: JObject,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    if listener.is_null() {
        s.clear_signer_request_listener();
        return;
    }
    let Ok(vm) = env.get_java_vm() else {
        return;
    };
    let Ok(global) = env.new_global_ref(&listener) else {
        return;
    };
    s.set_signer_request_listener(SignerRequestPushListener::new(vm, global));
}

/// Clear the JNI signer-request push listener without freeing the session
/// (issue #1284).
///
/// D6: a null/dead handle is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeClearSignerRequestListener(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        s.clear_signer_request_listener();
    }
}
