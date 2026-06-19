//! Canonical, app-agnostic feed C-ABI (ADR-0061).
//!
//! Three generic verbs replace the per-shape, app-named open/close symbols and
//! the per-call `nmp_app_load_older_feed` trigger:
//!
//! * [`nmp_app_open_feed`] — open by canonical descriptor JSON; returns the
//!   deterministic feed key (and ONLY the key — projection data stays push-only,
//!   ADR-0039).
//! * [`nmp_app_close_feed`] — forget the feed's viewport bookkeeping.
//! * [`nmp_app_set_feed_viewport`] — report raw viewport facts; NMP owns the
//!   pagination decision (Option B: auto-extend from declared viewport) and
//!   drives the EXISTING pull pager.
//!
//! The shell sends viewport facts and renders; it owns no cursor, no
//! `has_more` branch, no page size, no threshold. Those live in the
//! `nmp_feed::surface::FeedSurface` the composition root installs profiles +
//! openers into.
//!
//! D6 — every entry point is fire-and-forget. Null pointers, non-UTF-8 / empty
//! arguments, malformed JSON, and poisoned locks degrade to a safe no-op
//! (`open` returns NULL) rather than panicking across the FFI; a `catch_unwind`
//! guards the body as a belt-and-braces backstop.

use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use crate::{app_ref, c_string_argument, NmpApp};

/// Open a feed by canonical descriptor JSON.
///
/// Returns a heap-owned NUL-terminated feed-key string the caller MUST release
/// via [`crate::nmp_free_string`]. Returns NULL on a null `app`, a non-UTF-8 /
/// empty descriptor, or a malformed descriptor (fail closed) — never feed
/// state. The same descriptor always yields the same key (Rust / C-ABI / wasm
/// agree).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_open_feed(
    app: *mut NmpApp,
    descriptor_json: *const c_char,
) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(app) = app_ref(app) else {
            return ptr::null_mut();
        };
        let Some(descriptor) = c_string_argument(descriptor_json) else {
            return ptr::null_mut();
        };
        match app.open_feed(&descriptor) {
            Some(handle) => CString::new(handle.key.0)
                .map(CString::into_raw)
                .unwrap_or(ptr::null_mut()),
            None => ptr::null_mut(),
        }
    }))
    .unwrap_or(ptr::null_mut())
}

/// Close a feed by key. Idempotent — an unknown key is a harmless no-op.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_close_feed(app: *mut NmpApp, feed_key: *const c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(app) = app_ref(app) else {
            return;
        };
        let Some(key) = c_string_argument(feed_key) else {
            return;
        };
        let _ = app.close_feed(&key);
    }));
}

/// Report viewport facts for a feed. `viewport_json` is a
/// [`nmp_feed::FeedViewportIntent`] (`{firstVisible,lastVisible,renderedLen}`).
/// NMP decides whether to drive the pull pager; the shell branches on nothing.
/// Null / malformed input is a silent no-op.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_set_feed_viewport(
    app: *mut NmpApp,
    feed_key: *const c_char,
    viewport_json: *const c_char,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(app) = app_ref(app) else {
            return;
        };
        let Some(key) = c_string_argument(feed_key) else {
            return;
        };
        let Some(viewport) = c_string_argument(viewport_json) else {
            return;
        };
        let Some(intent) =
            serde_json::from_str::<nmp_feed::FeedViewportIntent>(&viewport).ok()
        else {
            return;
        };
        let _ = app.set_feed_viewport(&key, intent);
    }));
}
