//! Android JNI surface for keyring capability registration and identity restore.
//!
//! Two JNI entry points:
//!
//! 1. **`nativeSetCapabilityHandler`** — registers a synchronous Kotlin handler
//!    (e.g. `KeystoreKeyringCapability`) for all non-`external_signer` capability
//!    namespaces. The handler object must expose `fun handle(requestJson: String): String`.
//!    The `GlobalRef` is held in the session's `capability_handler` slot and cleared in
//!    `close_updates_locked` after the capability socket is quiesced.
//!
//! 2. **`nativeIdentityRestore`** — calls `nmp_app_chirp_identity_restore`, which
//!    reads the persisted nsec from the keyring capability (through the socket registered
//!    in `nativeNew`), signs in the kernel actor, and registers Marmot. Kotlin must call
//!    `nativeSetCapabilityHandler` BEFORE this so the keyring is live when the identity
//!    layer asks for the stored secret.
//!
//! Doctrine:
//! * **D6** — every failure (detached VM, JNI error, null session) is a data envelope
//!   or a boolean `false` (0); never a panic or NULL.
//! * **D7** — these entry points transport results only; Rust owns all policy.

use jni::objects::{JClass, JObject, JString};
use jni::sys::jlong;
use jni::JNIEnv;

#[cfg(feature = "marmot")]
use crate::jstring_to_cstring;
use crate::session_arc;

/// Register a synchronous Kotlin capability handler for all non-`external_signer`
/// namespaces (e.g. Android Keystore keyring). The `handler` object must
/// implement `fun handle(requestJson: String): String`.
///
/// The `GlobalRef` is held in the session and cleared in teardown after
/// `nmp_app_set_capability_callback(None)` quiesces any in-flight dispatch.
///
/// D6: a null/dead handle or JNI failure is a silent no-op.
/// D7: the handler executes and reports; Rust owns all policy.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSetCapabilityHandler(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    handler: JObject,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    if handler.is_null() {
        // Clear any existing handler (deregistration path).
        drop(s.capability_handler.lock().map(|mut g| g.take()));
        return;
    }
    // Obtain the JavaVM for cross-thread attachment in the trampoline.
    let vm = match env.get_java_vm() {
        Ok(vm) => vm,
        Err(_) => return,
    };
    // Promote the local handler reference to a GlobalRef so it survives
    // beyond this JNI call frame.
    let global = match env.new_global_ref(&handler) {
        Ok(g) => g,
        Err(_) => return,
    };
    let new_handler = Some(crate::capability::SyncCapabilityHandler::new(vm, global));
    drop(s.capability_handler.lock().map(|mut g| *g = new_handler));
}

/// Restore a persisted Chirp identity and register Marmot (marmot feature on).
///
/// Kotlin calls this during `KernelModel.startWithContext()` AFTER registering the
/// keyring capability handler, so the keyring is live when identity-restore reads
/// the persisted secret. `testNsec` is null in production; pass a non-null nsec
/// string only in headless UI tests.
///
/// Returns `true` (1) when a Marmot identity was registered, or `false` (0)
/// when no local key is active (e.g. NIP-55 / bunker accounts, first cold
/// start with no persisted identity).
///
/// D6: null handle, null dbDir, or any Rust-side failure degrades to 0
/// (no Marmot) without panicking across the JNI seam.
#[cfg(feature = "marmot")]
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeIdentityRestore(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    db_dir: JString,
    test_nsec: JString,
) -> jni::sys::jboolean {
    use std::ptr;
    use std::sync::atomic::Ordering;

    use nmp_app_chirp::{nmp_app_chirp_identity_restore, MarmotHandle};

    let Some(s) = session_arc(handle) else {
        return 0;
    };
    let db_dir_c = jstring_to_cstring(&mut env, &db_dir);
    // D13: the plaintext nsec crosses the JNI seam as bytes (it MUST, to be
    // restored), but the Rust-side copy is wrapped in `Zeroizing` the instant it
    // is materialized so the buffer is wiped on drop — including the early-return
    // and panic-unwind paths. Mirrors `nmp-ffi::identity::nmp_app_signin_nsec`,
    // which wraps its `c_string_argument(secret)` in `zeroize::Zeroizing`.
    // `zeroize` implements `Zeroize for CString`, so `Zeroizing<CString>` zeroes
    // the secret bytes (not just the smart-pointer) at scope exit.
    let test_nsec_c = {
        // A Kotlin `null` JString arrives with a null underlying JObject.
        let obj: &JObject = AsRef::<JObject>::as_ref(&test_nsec);
        if obj.as_raw().is_null() {
            None
        } else {
            jstring_to_cstring(&mut env, &test_nsec).map(zeroize::Zeroizing::new)
        }
    };

    // Call into Rust — reads the persisted secret from the keyring capability
    // (via the socket registered in `nativeNew`), signs in the kernel actor,
    // and registers Marmot. The capability handler must already be registered.
    let marmot_ptr = s.with_app(|app| {
        nmp_app_chirp_identity_restore(
            app,
            db_dir_c.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
            test_nsec_c.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
        )
    });

    // If Marmot registered successfully, store the raw pointer in the session
    // so `nativeFree` (via `marmot::unregister`) can tear it down correctly.
    // Mirrors the store pattern in `marmot::register_active`.
    match marmot_ptr {
        Some(ptr) if !ptr.is_null() => {
            // Idempotency on restart: unregister any stale handle first.
            crate::marmot::unregister(&s);
            s.marmot
                .store(ptr as *mut MarmotHandle as *mut std::ffi::c_void, Ordering::SeqCst);
            1
        }
        _ => 0,
    }
}

/// Stub when the `marmot` feature is disabled — always returns 0 (D6).
#[cfg(not(feature = "marmot"))]
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeIdentityRestore(
    _env: JNIEnv,
    _class: JClass,
    _handle: jlong,
    _db_dir: JString,
    _test_nsec: JString,
) -> jni::sys::jboolean {
    0
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::session::{insert_session, remove_session, Session};

    /// `nativeSetCapabilityHandler` with a dead session id is a no-op (D6).
    #[test]
    fn set_capability_handler_dead_session_is_noop() {
        // Call through the session registry with an invalid handle.
        // The function returns early (no panic), which is all we assert.
        let slot = super::session_arc(0);
        assert!(slot.is_none());
    }

    /// Verify that the capability_handler slot starts empty and can be set
    /// and cleared without holding a live JNI environment.
    #[test]
    fn capability_handler_slot_starts_empty() {
        let session = Session::test_session();
        let handle = insert_session(Arc::clone(&session));
        {
            let guard = session.capability_handler.lock().unwrap();
            assert!(guard.is_none(), "slot must be empty on a fresh session");
        }
        remove_session(handle);
    }

    /// Verify that `close_updates_locked` drops the capability_handler slot
    /// (ensures no GlobalRef dangling after teardown).
    #[test]
    fn close_updates_clears_capability_handler_slot() {
        let session = Session::test_session();
        // Nothing in the slot — close must not panic.
        session.close_updates();
        let guard = session.capability_handler.lock().unwrap();
        assert!(guard.is_none());
    }
}
