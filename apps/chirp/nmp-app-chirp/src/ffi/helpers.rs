//! Small shared helpers for the Chirp FFI surface: a null-aware C-string
//! reader and the typed action-body POD structs used by the social-verb
//! `ActionModule` impls in [`super::actions`].

use std::ffi::{c_char, CStr};

pub(super) fn c_string_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` (when non-null) is a valid
    // nul-terminated C string for the duration of this call.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(std::borrow::ToOwned::to_owned)
}

/// `chirp.react` action body: `{"target_event_id":"<hex>","reaction":"+"}`.
/// `reaction` defaults to `"+"` (the standard kind:7 like) when absent —
/// matching the old `nmp_app_react` FFI symbol's `unwrap_or("+")` behaviour.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(super) struct ReactAction {
    pub(super) target_event_id: String,
    #[serde(default = "default_reaction")]
    pub(super) reaction: String,
}

pub(super) fn default_reaction() -> String {
    "+".to_string()
}

/// `nmp.follow` / `nmp.unfollow` action body: `{"pubkey":"<hex>"}`.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(super) struct PubkeyAction {
    pub(super) pubkey: String,
}
