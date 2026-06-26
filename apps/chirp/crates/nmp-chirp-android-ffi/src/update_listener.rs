//! JNI push listener for kernel update frames (issue #614 — D8 no-polling).
//!
//! Extracted from `session.rs` to keep that file under the 500-LOC ceiling.
//! Mirrors the synchronous [`crate::capability::SyncCapabilityHandler`]
//! JNI-upcall pattern and the gallery's `android_push::GalleryUpdateListener`.
//!
//! NOTE (M14-0 / issue #2129): `nativeSetUpdateListener` and
//! `nativeClearUpdateListener` have been **deleted** — the app-loop update
//! lane is now served by the UniFFI `AppHandle::set_update_sink` /
//! `clear_update_sink` path in `uniffi_app_loop.rs`. The `UpdatePushListener`
//! type and `UpdateListenerSlot` typedef are retained because the residual JNI
//! lanes (capability, signer) still reference `session_arc` which
//! transitively requires the same session data.

use std::sync::{Arc, Mutex};

use jni::objects::GlobalRef;
use jni::JavaVM;

/// JNI push listener for kernel update frames.
///
/// Registered by `nativeSetUpdateListener`; invoked from the `on_update`
/// trampoline (`session.rs`) on the kernel's listener thread; cleared in
/// `Session::close_updates_locked` AFTER the update-callback quiescence gate
/// guarantees no further `on_update` invocations can run.
///
/// UAF safety: the load-bearing guard is the quiescence call
/// `nmp_app_set_update_callback(…, None)` in `close_updates_locked`, which does
/// NOT return until any in-flight `on_update` has completed. After it returns,
/// `on_update` can never touch the listener slot again, so dropping this
/// `GlobalRef` afterwards cannot race a live upcall.
pub(crate) struct UpdatePushListener {
    vm: JavaVM,
    handler: GlobalRef,
}

// SAFETY: `JavaVM` is `Send + Sync`. `GlobalRef` is safe to send across threads
// because the JVM tracks it; we only dereference it under `attach_current_thread`.
unsafe impl Send for UpdatePushListener {}
unsafe impl Sync for UpdatePushListener {}

impl UpdatePushListener {
    pub(crate) fn new(vm: JavaVM, handler: GlobalRef) -> Self {
        Self { vm, handler }
    }

    /// Invoke `listener.onUpdate(frame: ByteArray)` on the Kotlin listener.
    ///
    /// Runs inside `with_local_frame` so the JNI local-reference table is
    /// reclaimed on every push (dispatches on an already-attached thread never
    /// detach, which would otherwise leak the `ByteArray` local each call). D6:
    /// every failure (detached VM, JNI error/exception) is swallowed — this
    /// callback never panics across the JNI seam.
    pub(crate) fn push(&self, bytes: &[u8]) {
        let mut env = match self.vm.attach_current_thread() {
            Ok(env) => env,
            Err(_) => return,
        };
        let _ = env.with_local_frame(8, |env| -> Result<(), jni::errors::Error> {
            let array = env.byte_array_from_slice(bytes)?;
            env.call_method(
                &self.handler,
                "onUpdate",
                "([B)V",
                &[jni::objects::JValueGen::Object(array.as_ref())],
            )?;
            Ok(())
        });
        // Clear any pending JNI exception so the thread isn't left poisoned (D6).
        let _ = env.exception_clear();
    }
}

/// Session-owned slot for the JNI push update listener.
///
/// Wrapped in `Arc` so `on_update` can snapshot the listener reference,
/// drop the lock, and then invoke `push` without holding the mutex across
/// the JNI boundary (deadlock prevention — see session.rs `on_update`).
pub(crate) type UpdateListenerSlot = Mutex<Option<Arc<UpdatePushListener>>>;

// nativeSetUpdateListener and nativeClearUpdateListener were deleted in
// M14-0 (issue #2129).  Update delivery for the app-loop lane is now served
// by the UniFFI `AppHandle::set_update_sink` / `clear_update_sink` path in
// `uniffi_app_loop.rs`.  KernelBridge.kt no longer declares `nativeSetUpdateListener`
// or `nativeClearUpdateListener` external declarations — see bridge_parity.rs.

