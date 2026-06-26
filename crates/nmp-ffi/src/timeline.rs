//! Timeline / profile FFI wrappers — generic `open_interest`/`close_interest`
//! (the M2 feed seam, ADR-0042), `nostr:` URI routing, and profile claim/release.
//!
//! V-68 / V-112 (ADR-0042): `nmp_app_open_author`, `nmp_app_close_author`,
//! `nmp_app_open_thread`, `nmp_app_close_thread` deleted here; apps now open
//! typed feed sessions through `nmp_app_open_feed`.
//!
//! Split out of `ffi/mod.rs` to keep both files under the 300-LOC soft cap.
//! These reuse the parent module's validated-argument helpers (`app_ref`,
//! `c_string_argument`) and the shared `NmpApp` handle; the symbols stay
//! `#[no_mangle] extern "C"` so the Swift bridge sees a flat C ABI regardless
//! of the Rust module split.

use super::{app_ref, c_string_argument, NmpApp};
use std::ffi::c_char;

/// M2 (ADR-0042) — register (or attach an owner to) a generic tailing feed
/// interest. The generic replacement for `nmp_app_open_author` /
/// `nmp_app_open_thread` / the deleted `nmp_app_open_firehose_tag`: the app
/// or protocol composition layer passes a verbatim NIP-01 REQ filter after it
/// has compiled any primary-kind feed declaration into acquisition kinds. The
/// substrate owns no app feed-kind policy; it only parses and refcounts the
/// supplied filter.
///
/// * `filter_json` — standard Nostr REQ filter, parsed kernel-side into an
///   `InterestShape`; the shape hash gives deterministic dedup across call
///   sites passing the same filter (regardless of JSON key/element ordering).
/// * `consumer_id` — refcount owner key. Multiple owners sharing the same
///   filter keep one live subscription until the last `close_interest`.
/// * `scope` — `0` = ActiveAccount (re-route on account switch),
///   `1` = Global (account-agnostic, e.g. a hashtag firehose).
///
/// FFI-clean (D6): a null argument is a silent no-op; a non-object
/// `filter_json` surfaces a diagnostic toast (via `NmpApp::show_toast`)
/// rather than a panic. D8: forwards to the actor; no polling, no sync wait.
#[no_mangle]
pub extern "C" fn nmp_app_open_interest(
    app: *mut NmpApp,
    filter_json: *const c_char,
    consumer_id: *const c_char,
    scope: u32,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(filter_json) = c_string_argument(filter_json) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    // D6 — reject a malformed filter at the boundary with an observable toast
    // rather than silently registering nothing. The dispatch arm re-parses and
    // treats `None` as a no-op, but surfacing the error here gives the host a
    // visible signal that its filter string was rejected.
    if nmp_planner::InterestShape::from_filter_json(&filter_json).is_none() {
        app.show_toast("open_interest: malformed filter JSON".to_string());
        return;
    }

    app.open_interest(filter_json, consumer_id, scope);
}

/// M2 (ADR-0042) — detach one owner from a feed interest registered via
/// [`nmp_app_open_interest`]. The live subscription is dropped when the last
/// owner leaves. The `(filter_json, consumer_id, scope)` triple must match the
/// open call (the kernel reconstructs the same registry slot from the
/// `InterestShape` hash). FFI-clean (D6): null/malformed arguments are silent
/// no-ops (a close of a non-existent slot is harmless).
#[no_mangle]
pub extern "C" fn nmp_app_close_interest(
    app: *mut NmpApp,
    filter_json: *const c_char,
    consumer_id: *const c_char,
    scope: u32,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(filter_json) = c_string_argument(filter_json) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };

    app.close_interest(filter_json, consumer_id, scope);
}

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
// To decode a `nostr:` URI to an event key, call `nmp_nip21_decode_uri` first.

// V-68 / V-112 (ADR-0042): nmp_app_close_author / nmp_app_close_thread deleted.
// Apps close typed feed sessions by the opaque handle returned from open_feed.
