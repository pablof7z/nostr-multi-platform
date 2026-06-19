//! JNI push listener for kernel update frames (issue #614 — D8 no-polling).
//!
//! Extracted from `session.rs` to keep that file under the 500-LOC ceiling.
//! Mirrors the synchronous [`crate::capability::SyncCapabilityHandler`]
//! JNI-upcall pattern and the gallery's `android_push::GalleryUpdateListener`.

use std::sync::{Arc, Mutex};

use jni::objects::{GlobalRef, JClass, JObject};
use jni::sys::jlong;
use jni::{JNIEnv, JavaVM};

use crate::session::session_arc;

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

/// Register (or clear) the JNI push listener for kernel update frames
/// (issue #614 — D8 no-polling; replaces the deleted `nativeNextUpdate`
/// blocking drain).
///
/// `listener` must implement `fun onUpdate(frame: ByteArray)`. Frames are
/// pushed from the kernel's update-listener thread (a Rust background thread),
/// so Kotlin must treat `onUpdate` as a background callback and marshal to the
/// main thread itself when needed. Pass `null` to deregister.
///
/// D6: a null/dead handle, or any JNI failure obtaining the `JavaVM` / global
/// ref, is a silent no-op — never panics across the seam. The listener
/// `GlobalRef` is dropped on teardown (`nativeClose`/`nativeFree`) after the
/// update-callback quiescence gate guarantees no in-flight `on_update`.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSetUpdateListener(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    listener: JObject,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    if listener.is_null() {
        s.clear_push_listener();
        return;
    }
    let Ok(vm) = env.get_java_vm() else {
        return;
    };
    let Ok(global) = env.new_global_ref(&listener) else {
        return;
    };
    s.set_push_listener(UpdatePushListener::new(vm, global));
}

/// Clear the JNI push listener without freeing the session (issue #614).
///
/// D6: a null/dead handle is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeClearUpdateListener(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        s.clear_push_listener();
    }
}
