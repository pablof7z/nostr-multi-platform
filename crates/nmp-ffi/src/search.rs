//! Higher-order NIP-50 search C ABI wrappers.

use std::ffi::{c_char, c_int};

use super::{NmpApp, app_ref, c_string_argument};

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
    let descriptor = nmp_native_runtime::Nip50SearchSession::new(request, session_id);
    let _ = app.open_search_session(descriptor);
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
    let handle = nmp_native_runtime::Nip50SearchHandle::for_key(session_id);
    app.close_search_session(&handle);
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
    let handle = nmp_native_runtime::Nip50SearchHandle::for_key(session_id);
    let Some(bytes) = app.search_session_snapshot_bytes(&handle) else {
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
