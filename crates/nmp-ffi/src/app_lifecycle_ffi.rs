//! Test-only raw-pointer lifecycle helpers for `NmpApp`.

#[cfg(test)]
use std::ffi::c_void;
#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use nmp_core::__ffi_internal::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};

#[cfg(test)]
use crate::{app_ref, NmpApp};

#[cfg(test)]
pub(crate) type TestUpdateCallback = extern "C" fn(*mut c_void, *const u8, usize);

#[cfg(test)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub(crate) fn test_app_free(app: *mut NmpApp) {
    if !app.is_null() {
        unsafe {
            drop(Box::from_raw(app));
        }
    }
}

#[cfg(test)]
pub(crate) fn test_app_set_update_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<TestUpdateCallback>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let listener = callback.map(|callback| {
        let context = context as usize;
        Arc::new(move |bytes: &[u8]| {
            callback(context as *mut c_void, bytes.as_ptr(), bytes.len());
        }) as nmp_native_runtime::UpdateListener
    });
    app.set_update_listener(listener);
}

#[cfg(test)]
pub(crate) fn test_app_start(
    app: *mut NmpApp,
    visible_limit: std::ffi::c_uint,
    emit_hz: std::ffi::c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    app.start_runtime(clamp_visible(visible_limit), clamp_emit_hz(emit_hz));
}

#[cfg(test)]
pub(crate) fn test_app_reset(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.reset_runtime();
}

#[cfg(test)]
pub(crate) fn clamp_visible(visible_limit: std::ffi::c_uint) -> usize {
    if visible_limit == 0 {
        DEFAULT_VISIBLE_LIMIT
    } else {
        visible_limit.clamp(1, 500) as usize
    }
}

#[cfg(test)]
pub(crate) fn clamp_emit_hz(emit_hz: std::ffi::c_uint) -> u32 {
    if emit_hz == 0 {
        DEFAULT_EMIT_HZ
    } else {
        emit_hz.clamp(1, 12)
    }
}
