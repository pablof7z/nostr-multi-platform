//! C-ABI bridge for Rust-authored Chirp action specs.
//!
//! Native shells pass typed user intent JSON and receive a serialized
//! [`crate::action_specs::ActionDispatchSpec`] with the exact action namespace
//! and body JSON Rust wants dispatched through `nmp_app_dispatch_action`.

use std::ffi::{c_char, CStr, CString};

use crate::action_specs::action_spec_json_for_intent;

/// Build a Rust-owned Chirp action dispatch spec from typed intent JSON.
///
/// Returns `{"namespace":"...","body_json":"..."}` on success or
/// `{"error":"..."}` on malformed intent. The returned pointer must be freed
/// by the shell with `nmp_free_string`.
#[no_mangle]
pub extern "C" fn nmp_app_chirp_action_spec(intent_json: *const c_char) -> *mut c_char {
    let result = read_c_string(intent_json)
        .map(|intent| action_spec_json_for_intent(&intent))
        .unwrap_or_else(|| r#"{"error":"missing Chirp action intent JSON"}"#.to_string());
    CString::new(result)
        .unwrap_or_else(|_| CString::new(r#"{"error":"invalid action spec string"}"#).unwrap_or_default())
        .into_raw()
}

fn read_c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}
