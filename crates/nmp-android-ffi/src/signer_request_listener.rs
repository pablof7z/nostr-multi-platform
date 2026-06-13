//! JNI push listener for NIP-55 external-signer requests (issue #1284 — D8
//! no-polling).
//!
//! Direct twin of [`crate::update_listener::UpdatePushListener`]: it clones the
//! same GlobalRef-handler + JavaVM machinery so the kernel can push each
//! `ExternalSignerRequest` JSON straight to a registered Kotlin listener,
//! replacing the former 250 ms-timed `nativeNextSignerRequest` drain.

use std::sync::{Arc, Mutex};

use jni::objects::GlobalRef;
use jni::JavaVM;

/// JNI push listener for NIP-55 external-signer request JSON.
///
/// Registered by `nativeSetSignerRequestListener`; invoked from the
/// `on_capability_request` trampoline (`external_signer.rs`) on whichever Rust
/// thread dispatches the `external_signer` capability; cleared in
/// `Session::close_updates_locked` AFTER `nmp_app_set_capability_callback(…,
/// None)` unregisters the trampoline.
///
/// UAF safety: the load-bearing guard is the capability-socket unregister in
/// `close_updates_locked`. Unlike the synchronous capability handler (whose
/// `GlobalRef` is protected by the `capability_handler` mutex hold across the
/// upcall), this listener follows the *update-callback* shape: the trampoline
/// snapshots an `Arc` clone of the listener under the slot lock and drops the
/// lock before the upcall, so the `take()` in `close_updates_locked` only ever
/// races a cheap `Arc::clone`, never the JNI `push` itself. The dropped
/// `GlobalRef` is therefore never read by an in-flight upcall.
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
