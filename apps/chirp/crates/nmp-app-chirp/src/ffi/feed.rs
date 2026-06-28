//! The ONE public app-facing feed doorway (#1740 step 7).
//!
//! Apps open EVERY feed through a single typed entry: a JSON-encoded
//! [`nmp_feed::FeedParams`] in, an opaque [`nmp_feed::FeedHandle`] (its
//! `projection_key` + minted `session_id`) out. The handle is the ONLY token
//! `close_feed` needs — it never re-derives a filter or addresses a feed by a
//! hand-built key (D4). This is the single public app-facing way to open a feed
//! session; the raw active-follows declaration / contact-feed C symbols are
//! retired.
//!
//! ## Why this lives in the app-composition crate, not `nmp-ffi`
//!
//! [`nmp_ffi::NmpApp::open_feed`] takes a `compiler` — the scope→registration
//! step that names the OP-feed engine / follow set / typed sidecar. That wiring
//! lives in [`nmp_native_runtime::compile_feed_params`], ABOVE `nmp-ffi` in the DAG,
//! so `nmp-ffi` stays D0-clean (it matches on no `FeedScope` variant). The
//! generic C-ABI doorway therefore lives HERE — the composition layer that can
//! name both `NmpApp` and the compiler — and hands the same single canonical
//! compiler to `open_feed` for every scope. The per-app primary-kind decision
//! is the app's (it builds the `FeedParams`); wrapper/delete derivation is the
//! compiler's, below this boundary.
//!
//! ## Doctrine
//!
//! * **D0** — this crate is the composition point; `nmp-ffi` / `nmp-core` name
//!   no `FeedScope`. The compiler resolves scope semantics.
//! * **D4** — close/page address the session by HANDLE; no re-derived filter,
//!   no second feed engine. `open_feed` records the compiler's teardown recipe.
//! * **D6** — every entry is fail-closed: a null `app`, malformed params JSON,
//!   an invalid primary-kind declaration, or a fail-closed compile returns a
//!   typed `{"error":…}` (open) / silent no-op (close/page), never a panic
//!   across the ABI.

use std::ffi::{c_char, CString};

use nmp_feed::{FeedHandle, FeedParams};
use nmp_ffi::{FeedOpenError, NmpApp};

use super::helpers::c_string_opt;

/// The SINGLE compiler every public `open_feed` routes through: the closed
/// perspective compiler in `nmp-defaults`. `open_feed` validates primary kinds
/// and derives wrapper acquisition below this boundary before it runs.
fn compiler(
    app: &NmpApp,
    params: &FeedParams,
    kinds: &std::collections::BTreeSet<u32>,
) -> Result<nmp_feed::FeedSessionBuild, FeedOpenError> {
    nmp_native_runtime::compile_feed_params(app, params, kinds)
}

/// Serialize a minted handle to the `{"projection_key":"…","session_id":N}`
/// success envelope the host parses back into a handle for close/page.
#[must_use]
fn handle_json(handle: &FeedHandle) -> String {
    // `FeedHandle` is `Serialize`; the wire shape is its serde form. A serialize
    // failure is impossible for this POD shape, but fail closed to an error
    // envelope rather than unwrap (D6).
    serde_json::to_string(handle).unwrap_or_else(|_| r#"{"error":"handle_serialize"}"#.to_string())
}

/// A short, stable machine token for a typed [`FeedOpenError`] — emitted in the
/// `{"error":…}` envelope so a host can branch without parsing prose. The token
/// is the failure category, not a localized message (D6 — data, not English).
#[must_use]
fn open_error_token(err: &FeedOpenError) -> &'static str {
    match err {
        FeedOpenError::InvalidParams(_) => "invalid_primary_kinds",
        FeedOpenError::ScopeNotSupportedYet { .. } => "scope_unsupported",
        FeedOpenError::RegistryUnavailable => "registry_unavailable",
    }
}

/// Heap-allocate a JSON string for return across the C ABI. The caller MUST free
/// it via `nmp_free_string`. An interior-NUL (impossible for our JSON) collapses
/// to a fixed error envelope rather than a null return (D6 — non-null for a
/// non-null app).
#[must_use]
fn into_raw_json(json: String) -> *mut c_char {
    // Interior NUL is impossible for our JSON; if it ever occurred, fall back to a
    // static NUL-free error envelope, then to an empty string — never panic (D6).
    CString::new(json)
        .or_else(|_| CString::new(r#"{"error":"encode"}"#))
        .unwrap_or_default()
        .into_raw()
}

/// Open ONE feed session from a JSON-encoded [`FeedParams`] and return the
/// minted handle as JSON.
///
/// The ONLY public way to open a feed. `params_json` is a serialized
/// [`nmp_feed::FeedParams`]: the app's PRIMARY content kinds + a typed
/// [`nmp_feed::FeedScope`] acquisition + admission/ranking/window + projection
/// key. `open_feed` fail-closed-validates the primary kinds (rejecting wrapper
/// kinds 6/16 and delete kind 5), derives acquisition below this boundary, drives
/// the single canonical compiler, and records the teardown recipe under a minted
/// session id.
///
/// Returns a heap-owned C string the caller MUST free via `nmp_free_string`:
/// * success → `{"projection_key":"<key>","session_id":<u64>}` — feed this
///   verbatim to `nmp_app_close_feed`.
/// * failure → `{"error":"<token>"}` where token is `null_app`, `bad_params`,
///   `invalid_primary_kinds`, `scope_unsupported`, or `registry_unavailable`.
///
/// D6 — never NULL for a non-null `app`; never panics across the ABI.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_open_feed(app: *mut NmpApp, params_json: *const c_char) -> *mut c_char {
    if app.is_null() {
        return into_raw_json(r#"{"error":"null_app"}"#.to_string());
    }
    let Some(json) = c_string_opt(params_json) else {
        return into_raw_json(r#"{"error":"bad_params"}"#.to_string());
    };
    let Ok(params) = serde_json::from_str::<FeedParams>(&json) else {
        return into_raw_json(r#"{"error":"bad_params"}"#.to_string());
    };
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`,
    // live for this call. `open_feed` holds its own `Arc`s, not a borrow.
    let app_ref = unsafe { &*app };

    match app_ref.open_feed(&params, &compiler) {
        Ok(handle) => into_raw_json(handle_json(&handle)),
        Err(err) => into_raw_json(format!(r#"{{"error":"{}"}}"#, open_error_token(&err))),
    }
}

/// Close a feed session opened by [`nmp_app_open_feed`], addressed by its HANDLE.
///
/// `handle_json` is the verbatim `{"projection_key":…,"session_id":…}` envelope
/// the matching open returned. Tears down the session's controller, projection,
/// observer, and internal interests idempotently — never re-derives a filter
/// (D4). D6 — a null `app`, malformed handle, or already-closed session is a
/// harmless no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_close_feed(app: *mut NmpApp, handle_json: *const c_char) {
    let Some(handle) = parse_handle(app, handle_json) else {
        return;
    };
    // SAFETY: see `nmp_app_open_feed`.
    let app_ref = unsafe { &*app };
    let _ = app_ref.close_feed(&handle);
}

/// Shared handle decode for close: null-app and malformed-JSON both fail
/// closed to `None` (the caller no-ops).
fn parse_handle(app: *mut NmpApp, handle_json: *const c_char) -> Option<FeedHandle> {
    if app.is_null() {
        return None;
    }
    let json = c_string_opt(handle_json)?;
    serde_json::from_str::<FeedHandle>(&json).ok()
}

#[cfg(test)]
#[path = "feed/tests.rs"]
mod tests;
