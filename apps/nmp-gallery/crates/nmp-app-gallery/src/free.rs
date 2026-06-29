//! `nmp_app_gallery_free_string` — release a C string produced by a gallery C-ABI entry point.
//!
//! The gallery produces heap-allocated C strings from `nmp_app_gallery_snapshot_json_from_update_frame`
//! and `nmp_app_gallery_dispatch_action_bytes`. The caller must free them via this entry point.
//! The symbol is app-scoped so iOS callers cannot accidentally keep depending on the deleted
//! reusable `nmp-ffi` free helper.

use std::ffi::{c_char, CString};

/// Release a C string allocated by a gallery C-ABI entry point.
///
/// D6: a null pointer is a silent no-op.
///
/// # Safety
/// `ptr` must be a non-null pointer previously returned by a gallery C-ABI function
/// (specifically `nmp_app_gallery_snapshot_json_from_update_frame` or
/// `nmp_app_gallery_dispatch_action_bytes`), or null. Passing any other pointer is
/// undefined behaviour.
#[no_mangle]
pub unsafe extern "C" fn nmp_app_gallery_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}
