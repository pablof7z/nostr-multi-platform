use std::ffi::c_void;
use std::sync::Arc;
use std::time::Duration;

pub use nmp_native_runtime::{FeedOpenError, NmpApp};

pub fn new_app_ptr() -> *mut NmpApp {
    Box::into_raw(Box::new(nmp_native_runtime::new_app()))
}

pub fn free_app_ptr(app: *mut NmpApp) {
    if app.is_null() {
        return;
    }
    // SAFETY: callers pass ownership of a pointer returned by `new_app_ptr`.
    unsafe {
        (&*app).stop_runtime();
        drop(Box::from_raw(app));
    }
}

pub fn stop_app(app: *mut NmpApp) {
    if app.is_null() {
        return;
    }
    // SAFETY: callers pass a live pointer returned by `new_app_ptr`.
    unsafe { (&*app).stop_runtime() };
}

pub fn start_app(app: *mut NmpApp, visible_limit: u32, emit_hz: u32) {
    if app.is_null() {
        return;
    }
    // SAFETY: callers pass a live pointer returned by `new_app_ptr`.
    unsafe { (&*app).start_runtime(visible_limit as usize, emit_hz) };
}

pub fn set_c_update_listener(
    app: *mut NmpApp,
    ctx: *mut c_void,
    callback: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
) {
    if app.is_null() {
        return;
    }
    let listener = callback.map(|callback| {
        let ctx_addr = ctx as usize;
        Arc::new(move |payload: &[u8]| {
            callback(ctx_addr as *mut c_void, payload.as_ptr(), payload.len());
        }) as nmp_native_runtime::UpdateListener
    });
    // SAFETY: callers pass a live pointer returned by `new_app_ptr`.
    unsafe { (&*app).set_update_listener(listener) };
}

pub fn signin_nsec(app: *mut NmpApp, secret: &str, make_active: bool) {
    if app.is_null() {
        return;
    }
    // SAFETY: callers pass a live pointer returned by `new_app_ptr`.
    unsafe { (&*app).signin_nsec_for_test(secret.to_owned(), make_active) };
}

#[must_use]
pub fn inject_signed_event_json(app: *mut NmpApp, event_json: &str) -> bool {
    if app.is_null() {
        return false;
    }
    // SAFETY: callers pass a live pointer returned by `new_app_ptr`.
    unsafe { (&*app).inject_signed_event_json_for_test(event_json) }
}

#[must_use]
pub fn wait_barrier(app: *mut NmpApp, timeout_ms: u64) -> bool {
    if app.is_null() {
        return false;
    }
    // SAFETY: callers pass a live pointer returned by `new_app_ptr`.
    unsafe { (&*app).wait_barrier_for_test(Duration::from_millis(timeout_ms)) }
}

pub fn remove_account(app: *mut NmpApp, identity_id: &str) {
    if app.is_null() {
        return;
    }
    // SAFETY: callers pass a live pointer returned by `new_app_ptr`.
    unsafe { (&*app).remove_account(identity_id.to_owned()) };
}
