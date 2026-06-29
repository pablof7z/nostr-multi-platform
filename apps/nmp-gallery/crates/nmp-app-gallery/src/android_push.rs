//! JNI push listeners for gallery update frames and NIP-55 signer requests
//! (issue #614 / issue #1612 — D8 no-polling).
//!
//! Both are extracted here so `android.rs` stays under the 500-LOC ceiling.
//!
//! Teardown / UAF safety
//! ---------------------
//! Update listener: the gallery `on_update` trampoline reads the listener slot
//! under the [`GalleryUpdateCtx`] mutex; `nativeFree` calls
//! `NmpApp::set_update_listener(None)` (the quiescence gate — it blocks
//! until any in-flight `on_update` returns) BEFORE the `GalleryUpdateCtx` box
//! is dropped. So the `GlobalRef` is never dropped while a push is live.
//!
//! Signer-request listener: the capability handler snapshots
//! an `Arc` clone of the listener under the slot lock, drops the lock, THEN
//! invokes `push`. So `set_signer_request_listener` / teardown can only race a
//! cheap `Arc::clone`, never the JNI call itself. The capability handler is
//! cleared before `nativeFree` drops the session arc, so the `GlobalRef` is
//! never dropped during a live push.

use std::sync::{Arc, Mutex};

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
/// Session-owned update listener context. Holds the JNI push listener slot.
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

/// JNI push listener for NIP-55 external-signer request JSON (issue #1612 —
/// D8 no-polling; replaces the deleted `nativeNextSignerRequest` blocking drain).
///
/// Registered by `nativeSetSignerRequestListener`; invoked from the
/// capability handler on whichever Rust thread dispatches the
/// `external_signer` capability.
///
/// UAF safety: the trampoline snapshots an `Arc` clone of the listener under
/// the slot lock and drops the lock before the JNI upcall, so
/// `clear_signer_request_listener` racing a push only ever races the clone, not
/// the push itself. See module-level doc for the teardown ordering.
pub(crate) struct SignerRequestPushListener {
    vm: JavaVM,
    handler: GlobalRef,
}

// SAFETY: `JavaVM` is `Send + Sync`. `GlobalRef` is JVM-tracked; only
// dereferenced under `attach_current_thread`.
unsafe impl Send for SignerRequestPushListener {}
unsafe impl Sync for SignerRequestPushListener {}

impl SignerRequestPushListener {
    pub(crate) fn new(vm: JavaVM, handler: GlobalRef) -> Self {
        Self { vm, handler }
    }

    /// Invoke `listener.onSignerRequest(requestJson: String)` on the Kotlin
    /// listener. Runs inside `with_local_frame` so the JNI local-ref table is
    /// reclaimed on every push. D6: every failure is swallowed — never panics
    /// across the JNI seam.
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
/// Wrapped in `Arc` so the capability handler can snapshot the listener
/// reference, drop the lock, and then invoke `push` without holding the mutex
/// across the JNI boundary (deadlock prevention — mirrors the update path).
pub(crate) type SignerRequestListenerSlot = Arc<Mutex<Option<Arc<SignerRequestPushListener>>>>;
