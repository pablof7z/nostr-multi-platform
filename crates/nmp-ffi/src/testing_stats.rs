//! Test-support FFI stats helpers.
//!
//! Split from `testing.rs` so injectors and diagnostics stay below the
//! repository file-size hard cap.

#![cfg(any(test, feature = "test-support"))]

use super::{app_ref, NmpApp};

/// Test-support — read the actor command lane's bounded-channel counters.
///
/// `out_depth` is the approximate accepted-command queue depth. `out_drops` is
/// the cumulative count of commands shed because the bounded actor inbox was
/// full. Either output pointer may be null.
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`. Not part of the
/// production FFI ABI.
#[no_mangle]
pub extern "C" fn nmp_app_read_command_lane_stats(
    app: *mut NmpApp,
    out_depth: *mut u64,
    out_drops: *mut u64,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    if !out_depth.is_null() {
        // SAFETY: non-null pointer checked above; caller guarantees the lifetime.
        unsafe {
            *out_depth = app.queue_depth_for_test();
        }
    }
    if !out_drops.is_null() {
        // SAFETY: non-null pointer checked above; caller guarantees the lifetime.
        unsafe {
            *out_drops = app.command_drops_for_test();
        }
    }
}
