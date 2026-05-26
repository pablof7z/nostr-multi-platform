//! Generic feed viewport FFI.
//!
//! App crates register reusable feed controllers on an `NmpApp`; native shells
//! report viewport intent by key. The controller and page policy live in NMP.

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
