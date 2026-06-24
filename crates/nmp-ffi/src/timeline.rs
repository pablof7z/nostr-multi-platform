//! Timeline / profile FFI wrappers — generic `open_interest`/`close_interest`
//! (the M2 feed seam, ADR-0042), `nostr:` URI routing, and profile claim/release.
//!
//! V-68 / V-112 (ADR-0042): `nmp_app_open_author`, `nmp_app_close_author`,
//! `nmp_app_open_thread`, `nmp_app_close_thread` deleted here; apps now call
//! their own per-app seam (e.g. `nmp_app_chirp_open_author_feed`) which
//! registers a `FlatFeed` and calls `nmp_app_open_interest` for kernel admission.
//!
//! Split out of `ffi/mod.rs` to keep both files under the 300-LOC soft cap.
//! These reuse the parent module's validated-argument helpers (`app_ref`,
//! `c_string_argument`) and the shared `NmpApp` handle; the symbols stay
//! `#[no_mangle] extern "C"` so the Swift bridge sees a flat C ABI regardless
//! of the Rust module split.

use super::{NmpApp, app_ref, c_string_argument};
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
//   nmp_app_resolve_ref(app, 1/*event*/, key, consumer_id, 2/*embed*/, 0/*cache_ok*/)
//   nmp_app_release_ref(app, 1/*event*/, key, consumer_id)
// The `key` is the event-id hex (for nevent/note) or `"kind:pubkey:d"` (for naddr).
// To decode a `nostr:` URI to an event key, call `nmp_nip21_decode_uri` first.

// V-68 / V-112 (ADR-0042): nmp_app_close_author / nmp_app_close_thread deleted.
// Apps use their per-app seam (nmp_app_chirp_close_author_feed etc.) which
// releases the FlatFeed and calls nmp_app_close_interest for kernel cleanup.

/// Parse a primary-kinds JSON string (e.g. `"[1]"`) into a `BTreeSet<u32>`.
///
/// Returns `None` on the first invalid element (negative integer, float,
/// string, null, or a value > u32::MAX). An empty array `[]` is a legitimate
/// clear and returns `Some(BTreeSet::new())` without further validation.
/// A non-array top-level value also returns `None`.
///
/// Duplicates within a valid array are silently deduplicated by the set
/// semantics — this is intentional and consistent with the kernel's registry.
pub(crate) fn parse_primary_kinds_json(s: &str) -> Option<std::collections::BTreeSet<u32>> {
    let arr = serde_json::from_str::<serde_json::Value>(s)
        .ok()
        .and_then(|v| v.as_array().cloned())?;
    if arr.is_empty() {
        // Legitimate clear — skip element validation.
        return Some(std::collections::BTreeSet::new());
    }
    let mut set = std::collections::BTreeSet::new();
    for element in &arr {
        match element.as_u64().and_then(|n| u32::try_from(n).ok()) {
            // Repost wrappers (6/16) and the delete kind (5) are compiler-derived
            // acquisition, never primary app input (issue #1740 step 5) — reject
            // them here so the app-facing parser fails closed, not only the lower
            // `NmpApp` call.
            Some(k) if !nmp_nip18::is_repost_kind(k) && k != nmp_nip18::KIND_DELETE => {
                set.insert(k);
            }
            None => return None,    // first invalid element → bail
            Some(_) => return None, // repost wrappers / delete kind are derived, not primary
        }
    }
    Some(set)
}

/// Declare the active-account-follows feed from app primary content kinds.
///
/// `primary_kinds_json` is a JSON array of unsigned 32-bit integers identifying
/// the primary content kinds the app wants to render, e.g. `"[1]"` for Chirp.
/// Repost wrappers (`6` for primary kind `1`, `16` for every non-kind-1 primary
/// target) and the delete kind (`5`) are derived acquisition, so declaring them
/// as primary fails closed here before the compiled acquisition set is sent to
/// `nmp-core`. An empty array `"[]"` is a legitimate clear. A malformed or
/// non-array value, or any element that is not a non-negative integer fitting
/// in u32, surfaces a diagnostic toast rather than a panic or silent
/// registration (D6).
///
/// #1740 step 8: this is INTERNAL composition glue, NOT a public app surface.
/// The raw `nmp_app_open_contact_feed` C-ABI shim that used to delegate here is
/// DELETED; the only public way to open the active-follows feed is the typed
/// `nmp_app_open_feed(FeedScope::ActiveUserFollows)` doorway, whose compiler arm
/// (`compile_active_user_follows`) drives the `NmpApp::declare_active_follows_feed`
/// method. This helper survives only for the home-feed wiring path; no public
/// `nmp_app_*` symbol delegates to it.
pub fn declare_active_follows_feed(app: *mut NmpApp, primary_kinds_json: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(kinds_str) = c_string_argument(primary_kinds_json) else {
        return;
    };

    let primary_kinds = match parse_primary_kinds_json(&kinds_str) {
        Some(set) => set,
        None => {
            app.show_toast("declare_active_follows_feed: malformed primary kinds JSON".to_string());
            return;
        }
    };
    let _ = app.declare_active_follows_feed(primary_kinds);
}

/// Clear the active-account-follows feed declaration.
///
/// Withdraws all follow-feed M2 interests from the lifecycle registry;
/// `drain_lifecycle_tick` emits CLOSE frames for any live REQs on the next
/// idle tick. D6: a null `app` is a silent no-op. #1740 step 8: INTERNAL
/// composition glue only — the raw `nmp_app_close_contact_feed` C-ABI shim that
/// delegated here is DELETED; feed close is handle-based via `nmp_app_close_feed`.
pub fn clear_active_follows_feed(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.clear_active_follows_feed();
}

// #1740 step 8: the raw `nmp_app_open_contact_feed` / `nmp_app_close_contact_feed`
// C-ABI shims are DELETED. The only public way to open the active-follows feed is
// the typed `nmp_app_open_feed(FeedScope::ActiveUserFollows)` doorway in the
// app-composition crate; close is HANDLE-based (`nmp_app_close_feed`). The
// `declare_active_follows_feed` / `clear_active_follows_feed` Rust helpers below
// stay as INTERNAL composition glue (the home-feed wiring + the perspective
// compiler's `ActiveUserFollows` arm drive them), never as a public C symbol.

#[cfg(test)]
mod kinds_parse_tests {
    use super::parse_primary_kinds_json;
    use std::collections::BTreeSet;

    #[test]
    fn valid_primary_kinds_parsed_and_deduped() {
        let result = parse_primary_kinds_json("[1, 20, 1]");
        assert_eq!(
            result,
            Some(BTreeSet::from([1u32, 20u32])),
            "duplicate elements must be deduped by BTreeSet"
        );
    }

    #[test]
    fn empty_array_is_legitimate_clear() {
        let result = parse_primary_kinds_json("[]");
        assert_eq!(
            result,
            Some(BTreeSet::new()),
            "empty array must yield an empty set (legitimate clear)"
        );
    }

    #[test]
    fn negative_element_is_rejected() {
        let result = parse_primary_kinds_json("[1, -1, 6]");
        assert!(
            result.is_none(),
            "a negative element must cause parse_primary_kinds_json to return None"
        );
    }

    #[test]
    fn float_element_is_rejected() {
        let result = parse_primary_kinds_json("[1, 1.5, 6]");
        assert!(
            result.is_none(),
            "a float element must cause parse_primary_kinds_json to return None"
        );
    }

    #[test]
    fn string_element_is_rejected() {
        let result = parse_primary_kinds_json(r#"[1, "six", 6]"#);
        assert!(
            result.is_none(),
            "a string element must cause parse_primary_kinds_json to return None"
        );
    }

    #[test]
    fn null_element_is_rejected() {
        let result = parse_primary_kinds_json("[1, null, 6]");
        assert!(
            result.is_none(),
            "a null element must cause parse_primary_kinds_json to return None"
        );
    }

    #[test]
    fn value_above_u32_max_is_rejected() {
        // 4_294_967_296 = u32::MAX + 1. The old `n as u32` cast would have
        // wrapped this to 0 (kind 0), silently registering it.
        let result = parse_primary_kinds_json("[1, 4294967296, 6]");
        assert!(
            result.is_none(),
            "a value > u32::MAX must cause parse_primary_kinds_json to return None (was silently wrapping)"
        );
    }

    #[test]
    fn repost_wrapper_primary_kinds_are_rejected() {
        assert!(parse_primary_kinds_json("[1, 6]").is_none());
        assert!(parse_primary_kinds_json("[16]").is_none());
    }

    #[test]
    fn delete_kind_primary_is_rejected() {
        // kind:5 is compiler-derived suppression acquisition, never primary
        // app input — the app-facing parser must fail closed.
        assert!(parse_primary_kinds_json("[5]").is_none());
        assert!(parse_primary_kinds_json("[1, 5]").is_none());
    }

    #[test]
    fn non_array_top_level_is_rejected() {
        assert!(parse_primary_kinds_json(r#"{"kinds":[1,6]}"#).is_none());
        assert!(parse_primary_kinds_json("1").is_none());
        assert!(parse_primary_kinds_json("null").is_none());
    }

    #[test]
    fn primary_kind_1_expands_to_kind_6_and_delete_acquisition() {
        let primary = parse_primary_kinds_json("[1]").unwrap();
        assert_eq!(
            nmp_nip18::acquisition_kinds_for_primary(primary),
            BTreeSet::from([1u32, 6u32, nmp_nip18::KIND_DELETE])
        );
    }

    #[test]
    fn non_kind_1_primary_expands_to_kind_16_and_delete_acquisition() {
        let primary = parse_primary_kinds_json("[20]").unwrap();
        assert_eq!(
            nmp_nip18::acquisition_kinds_for_primary(primary),
            BTreeSet::from([16u32, 20u32, nmp_nip18::KIND_DELETE])
        );
    }
}
