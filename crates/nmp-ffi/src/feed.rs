//! Feed viewport-command FFI.
//!
//! Native shells do not open or close feed sessions through public C symbols.
//! App-owned Rust composition keeps typed `NmpApp::open_feed` helpers; this
//! module exposes only viewport commands that remain valid across the C ABI.

use std::ffi::c_char;

use crate::{app_ref, c_string_argument};

#[no_mangle]
pub extern "C" fn nmp_app_load_older_feed(app: *mut crate::NmpApp, key: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(key) = c_string_argument(key) else {
        return;
    };
    let _ = app.load_older_feed(&key);
}
