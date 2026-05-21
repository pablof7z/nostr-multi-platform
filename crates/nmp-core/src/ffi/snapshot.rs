//! FFI snapshot-projection registration entry point.
//!
//! [`nmp_app_register_snapshot_projection`] is the output-side counterpart to
//! [`super::action::nmp_app_register_action_executor`]. Where the action seam
//! lets a host *dispatch* a custom namespace, this seam lets a host *project*
//! a custom namespace into every snapshot.
//!
//! # The seam
//!
//! `KernelSnapshot` is a sealed social wire schema. A host registers a
//! **snapshot projector** — a C callback invoked on every snapshot tick whose
//! returned JSON string is appended to `KernelSnapshot::projections` under a
//! host-chosen key. A marketplace app registers `"market.listings"`, a todo
//! app `"todo.items"` — each gets its own namespace WITHOUT editing
//! `nmp-core`'s typed social fields.
//!
//! # Doctrine
//!
//! * **D6** — a null `app`, a null/empty/invalid `key`, or a null `projector`
//!   is a silent no-op. A bad registration argument never crashes the host.
//! * **D8** — the projector callback runs on the actor thread inside the
//!   snapshot tick. It MUST be cheap and non-blocking; a blocking projector
//!   stalls every subsequent snapshot.

use std::ffi::{c_char, CStr};

use super::{app_ref, c_string_argument, NmpApp};

/// Host-supplied snapshot projector callback.
///
/// Called on every snapshot tick. Returns a NUL-terminated JSON C string
/// contributed to the host's projection key, or `NULL` to contribute an empty
/// JSON object. The returned pointer is read immediately and copied into an
/// owned Rust value; the host owns its lifetime and may free or reuse it
/// after the callback returns.
///
/// A non-JSON / un-parseable return is treated as JSON `null` (D6: a bad
/// projector return is data, never a panic).
pub type NmpSnapshotProjector = unsafe extern "C" fn() -> *const c_char;

/// Register a host-supplied snapshot projector for `key` — the host-extensible
/// snapshot-output seam.
///
/// This is the C-ABI counterpart to [`NmpApp::register_snapshot_projection`]:
/// a host wires a snapshot namespace into the kernel **without editing
/// `nmp-core`**. The bridge closure invokes `projector`, parses its returned
/// JSON string, and the kernel appends the result under `key` in
/// `KernelSnapshot::projections` on every tick.
///
/// The projection registry lives behind a shared `Arc<Mutex<…>>` slot bound
/// onto the actor-thread-owned kernel; this call only takes `&NmpApp` (the
/// mutation is a lock-and-push), so it is safe to call concurrently with a
/// running actor. It is still intended as a host-init call.
///
/// A null `app`, a null/empty/invalid `key`, or a null `projector` is a
/// silent no-op (D6: a bad registration argument never crashes the host).
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
/// `key` must be a valid UTF-8 NUL-terminated C string (or null).
/// `projector`, when `Some`, must be a valid function pointer for the
/// remaining lifetime of `app` — the registry retains it.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_register_snapshot_projection(
    app: *mut NmpApp,
    key: *const c_char,
    projector: Option<NmpSnapshotProjector>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(key) = c_string_argument(key) else {
        return;
    };
    let Some(projector) = projector else {
        return;
    };
    app.register_snapshot_projection(key, move || {
        // SAFETY: `projector` is a valid function pointer per this symbol's
        // safety contract.
        let ptr = unsafe { projector() };
        if ptr.is_null() {
            // A NULL return contributes an empty JSON object.
            return serde_json::Value::Object(serde_json::Map::new());
        }
        // SAFETY: a non-null return is, per the callback contract, a valid
        // NUL-terminated C string live for the duration of this read. The
        // bytes are copied immediately; the host retains ownership.
        let json = unsafe { CStr::from_ptr(ptr) }.to_string_lossy();
        // D6: an un-parseable projector return collapses to JSON `null`
        // rather than panicking across the C ABI boundary.
        serde_json::from_str(&json).unwrap_or(serde_json::Value::Null)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{nmp_app_free, nmp_app_new};
    use std::ffi::CString;

    /// A registered C projector contributes a parsed JSON value under its key.
    /// Uses a `static` C string so the returned pointer outlives the call —
    /// the real ABI contract only requires it live for the read.
    extern "C" fn counter_projector() -> *const c_char {
        // `c"…"` literal: a `'static` NUL-terminated C string, valid for the
        // whole program — satisfies the projector-return lifetime contract.
        c"{\"count\":42}".as_ptr()
    }

    /// A projector returning NULL contributes an empty JSON object.
    extern "C" fn null_projector() -> *const c_char {
        std::ptr::null()
    }

    #[test]
    fn register_snapshot_projection_runs_c_projector() {
        let app = nmp_app_new();
        let key = CString::new("test.counter").unwrap();
        nmp_app_register_snapshot_projection(app, key.as_ptr(), Some(counter_projector));
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        let projections = app_ref.run_snapshot_projections_for_test();
        assert_eq!(
            projections.get("test.counter").and_then(|v| v.get("count")),
            Some(&serde_json::json!(42)),
            "C projector return must be parsed under its key"
        );
        nmp_app_free(app);
    }

    #[test]
    fn null_projector_return_contributes_empty_object() {
        let app = nmp_app_new();
        let key = CString::new("test.empty").unwrap();
        nmp_app_register_snapshot_projection(app, key.as_ptr(), Some(null_projector));
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        let projections = app_ref.run_snapshot_projections_for_test();
        assert_eq!(
            projections.get("test.empty"),
            Some(&serde_json::json!({})),
            "a NULL projector return is an empty JSON object"
        );
        nmp_app_free(app);
    }

    #[test]
    fn null_app_is_silent_noop() {
        let key = CString::new("test.counter").unwrap();
        // Must not panic / crash — D6.
        nmp_app_register_snapshot_projection(
            std::ptr::null_mut(),
            key.as_ptr(),
            Some(counter_projector),
        );
    }

    #[test]
    fn null_key_is_silent_noop() {
        let app = nmp_app_new();
        nmp_app_register_snapshot_projection(
            app,
            std::ptr::null(),
            Some(counter_projector),
        );
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        // A null key must register nothing — the registry contains only the
        // built-in `"wallet"` projection (`feature = "wallet"`), never the
        // test's `test.counter` key.
        assert!(
            !app_ref
                .run_snapshot_projections_for_test()
                .contains_key("test.counter"),
            "a null key must register nothing"
        );
        nmp_app_free(app);
    }

    #[test]
    fn null_projector_is_silent_noop() {
        let app = nmp_app_new();
        let key = CString::new("test.counter").unwrap();
        nmp_app_register_snapshot_projection(app, key.as_ptr(), None);
        // SAFETY: `nmp_app_new` never returns null.
        let app_ref = unsafe { &*app };
        // A null projector must register nothing — the registry never gains
        // the test's `test.counter` key (the built-in `"wallet"` projection
        // under `feature = "wallet"` may still be present).
        assert!(
            !app_ref
                .run_snapshot_projections_for_test()
                .contains_key("test.counter"),
            "a null projector must register nothing"
        );
        nmp_app_free(app);
    }
}
