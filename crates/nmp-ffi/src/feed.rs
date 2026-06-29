//! Generic typed feed-session FFI.
//!
//! Native shells open feeds by passing typed `FeedParams` JSON and receive a
//! serialized `FeedHandle`. The controller, compiler, and page policy live in
//! Rust/NMP.

use std::ffi::{CString, c_char};

use crate::{app_ref, c_string_argument};

#[no_mangle]
pub extern "C" fn nmp_app_open_feed(
    app: *mut crate::NmpApp,
    params_json: *const c_char,
) -> *mut c_char {
    let Some(app) = app_ref(app) else {
        return empty_c_string();
    };
    let Some(params_json) = c_string_argument(params_json) else {
        return empty_c_string();
    };
    let Ok((params, _)) = crate::decode_and_validate_feed_params(&params_json) else {
        return empty_c_string();
    };
    let Ok(handle) = app.open_feed(&params, &nmp_native_runtime::compile_feed_params) else {
        return empty_c_string();
    };
    let Ok(json) = serde_json::to_string(&handle) else {
        let _ = app.close_feed(&handle);
        return empty_c_string();
    };

    into_c_string(json)
}

#[no_mangle]
pub extern "C" fn nmp_app_close_feed(app: *mut crate::NmpApp, handle_json: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(handle_json) = c_string_argument(handle_json) else {
        return;
    };
    let Ok(handle) = serde_json::from_str::<crate::FeedHandle>(&handle_json) else {
        return;
    };

    let _ = app.close_feed(&handle);
}

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

fn into_c_string(value: String) -> *mut c_char {
    CString::new(value)
        .unwrap_or_else(|_| c"".to_owned())
        .into_raw()
}

fn empty_c_string() -> *mut c_char {
    c"".to_owned().into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{nmp_app_free, nmp_app_new};
    use std::ffi::{CStr, CString};

    fn params_json(projection: &str) -> CString {
        CString::new(format!(
            r#"{{
              "primary_kinds": [1],
              "acquisition": "ActiveUserFollows",
              "admission": "All",
              "ranking": "ChronologicalDesc",
              "window": {{ "initial_limit": 80 }},
              "projection": "{projection}"
            }}"#
        ))
        .expect("fixture has no NUL")
    }

    unsafe fn take_string(ptr: *mut c_char) -> String {
        let value = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        crate::nmp_free_string(ptr);
        value
    }

    #[test]
    fn open_feed_returns_serialized_handle_and_close_accepts_it() {
        let app = nmp_app_new();
        let params = params_json("ffi.feed.test");
        let handle_json = unsafe { take_string(nmp_app_open_feed(app, params.as_ptr())) };
        let handle: crate::FeedHandle =
            serde_json::from_str(&handle_json).expect("open_feed returns FeedHandle JSON");
        assert_eq!(handle.projection_key.0, "ffi.feed.test");

        let handle_c = CString::new(handle_json).expect("handle JSON has no NUL");
        nmp_app_close_feed(app, handle_c.as_ptr());
        let app_ref = crate::app_ref(app).expect("app");
        assert_eq!(app_ref.live_feed_session_count(), 0);
        nmp_app_free(app);
    }

    #[test]
    fn invalid_feed_params_fail_closed_with_empty_string() {
        let app = nmp_app_new();
        let invalid = CString::new(r#"{"primary_kinds":[6]}"#).unwrap();
        let handle_json = unsafe { take_string(nmp_app_open_feed(app, invalid.as_ptr())) };
        assert!(handle_json.is_empty());
        let app_ref = crate::app_ref(app).expect("app");
        assert_eq!(app_ref.live_feed_session_count(), 0);
        nmp_app_free(app);
    }
}
