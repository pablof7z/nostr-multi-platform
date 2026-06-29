//! Timeline / profile FFI wrappers — `nostr:` URI routing and profile
//! claim/release.
//!
//! V-68 / V-112 (ADR-0042): `nmp_app_open_author`, `nmp_app_close_author`,
//! `nmp_app_open_thread`, `nmp_app_close_thread` deleted here; apps now open
//! typed feed sessions through app-owned Rust helpers over `NmpApp::open_feed`.
//!
//! Split out of `ffi/mod.rs` to keep both files under the 300-LOC soft cap.
//! These reuse the parent module's validated-argument helpers (`app_ref`,
//! `c_string_argument`) and the shared `NmpApp` handle; the symbols stay
//! `#[no_mangle] extern "C"` so the Swift bridge sees a flat C ABI regardless
//! of the Rust module split.

use super::{NmpApp, app_ref, c_string_argument};
use std::ffi::c_char;

/// Open whatever a `nostr:` URI (or bare NIP-19 entity) points at (T95/T80).
/// Routed through the `KernelAction` reducer: success registers the resolved
/// interest + pushes `ViewOpened`, failure pushes `UriRejected`. FFI-clean
/// (D6): a null/invalid argument is a silent no-op, never a panic.
#[no_mangle]
pub extern "C" fn nmp_app_open_uri(app: *mut NmpApp, uri: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };

    app.open_uri(uri);
}

// Event URI front doors are removed (no compat shims). Callers use:
//   nmp_app_resolve_event_embed(app, key, consumer_id)
//   nmp_app_release_event_ref(app, key, consumer_id)
// The `key` is the event-id hex (for nevent/note) or `"kind:pubkey:d"` (for naddr).
// To decode a `nostr:` URI to an event key, use an app-owned URI adapter first.

// V-68 / V-112 (ADR-0042): nmp_app_close_author / nmp_app_close_thread deleted.
// Apps close typed feed sessions by the opaque handle returned from open_feed.
