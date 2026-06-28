//! Higher-order NIP-50 search C ABI wrappers.

use std::ffi::{c_char, c_int};

use super::{app_ref, c_string_argument, NmpApp};

/// Open a NIP-50 search session from a JSON query payload.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_search_open(
    app: *mut NmpApp,
    request_json: *const c_char,
    session_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(request_json) = c_string_argument(request_json) else {
        return;
    };
    let Some(session_id) = c_string_argument(session_id).filter(|s| !s.is_empty()) else {
        return;
    };
    let Some(request) = parse_search_request(&request_json) else {
        return;
    };
    let _ = app.open_search(request, &session_id);
}

/// Close a NIP-50 search session opened via [`nmp_app_search_open`].
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_search_close(app: *mut NmpApp, session_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(session_id) = c_string_argument(session_id).filter(|s| !s.is_empty()) else {
        return;
    };
    app.close_search(&session_id);
}

/// Copy the current typed `N50S` search-results buffer for a session.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_search_snapshot(
    app: *mut NmpApp,
    session_id: *const c_char,
    out_buf: *mut u8,
    cap: usize,
) -> c_int {
    let Some(app) = app_ref(app) else {
        return 0;
    };
    let Some(session_id) = c_string_argument(session_id).filter(|s| !s.is_empty()) else {
        return 0;
    };
    let Some(bytes) = app.search_snapshot_bytes(&session_id) else {
        return 0;
    };
    let needed = bytes.len();
    if !out_buf.is_null() && needed <= cap {
        // SAFETY: `out_buf` points to >= `cap` >= `needed` writable bytes per
        // the C ABI contract; `bytes` is a distinct owned Vec.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, needed);
        }
    }
    c_int::try_from(needed).unwrap_or(c_int::MAX)
}

pub(crate) fn parse_search_request(json: &str) -> Option<nmp_native_runtime::SearchRequest> {
    nmp_native_runtime::parse_search_request(json)
}
