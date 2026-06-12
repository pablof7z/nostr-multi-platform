//! JNI push listener for gallery update frames (issue #614 — D8 no-polling).
//!
//! Mirrors the Chirp `UpdatePushListener` in
//! `crates/nmp-android-ffi/src/session.rs`. Extracted into its own module so
//! `android.rs` stays under the 500-LOC ceiling.
//!
//! Teardown / UAF safety
//! ---------------------
//! The gallery `on_update` trampoline reads the listener slot under the
//! [`GalleryUpdateCtx`] mutex; `nativeFree` calls
//! `nmp_app_set_update_callback(…, None)` (the quiescence gate — it blocks until
//! any in-flight `on_update` returns) BEFORE the `GalleryUpdateCtx` box is
//! dropped. So the `GlobalRef` is never dropped while a push is live.

use std::sync::Mutex;

use jni::objects::GlobalRef;
use jni::JavaVM;

/// A Kotlin object implementing `fun onUpdate(frame: ByteArray)`.
/// Registered by `nativeSetUpdateListener`; cleared on teardown.
pub(crate) struct GalleryUpdateListener {
    vm: JavaVM,
    handler: GlobalRef,
}

// SAFETY: `JavaVM` is `Send + Sync`; `GlobalRef` is JVM-tracked and only
// dereferenced under `attach_current_thread`.
unsafe impl Send for GalleryUpdateListener {}
unsafe impl Sync for GalleryUpdateListener {}

impl GalleryUpdateListener {
    pub(crate) fn new(vm: JavaVM, handler: GlobalRef) -> Self {
        Self { vm, handler }
    }

    /// Invoke `listener.onUpdate(frame: ByteArray)`. Runs inside
    /// `with_local_frame` so the per-push `ByteArray` local ref is reclaimed.
    /// D6: every failure is swallowed — never panics across the JNI seam.
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
        let _ = env.exception_clear();
    }
}

/// Update-callback context owned by the [`crate::android`] `GallerySession`.
///
/// Boxed and passed as the `nmp_app_set_update_callback` context pointer. Holds
/// the JNI push listener slot. (Issue #614 removed the mpsc update channel; the
/// signer-request channel stays on its own boxed `Sender`.)
pub(crate) struct GalleryUpdateCtx {
    pub(crate) listener: Mutex<Option<GalleryUpdateListener>>,
}

impl GalleryUpdateCtx {
    pub(crate) fn new() -> Self {
        Self {
            listener: Mutex::new(None),
        }
    }

    /// Forward one frame to the registered listener, if any.
    pub(crate) fn push(&self, bytes: &[u8]) {
        if let Ok(guard) = self.listener.lock() {
            if let Some(listener) = guard.as_ref() {
                listener.push(bytes);
            }
        }
    }

    pub(crate) fn set_listener(&self, listener: GalleryUpdateListener) {
        if let Ok(mut slot) = self.listener.lock() {
            *slot = Some(listener);
        }
    }

    pub(crate) fn clear_listener(&self) {
        if let Ok(mut slot) = self.listener.lock() {
            slot.take();
        }
    }
}
