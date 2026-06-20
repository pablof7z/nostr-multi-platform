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

use super::{app_ref, c_string_argument, NmpApp};
use nmp_core::__ffi_internal::is_hex_pubkey;
use nmp_core::ActorCommand;
use std::ffi::{c_char, c_int};

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
/// `filter_json` surfaces a diagnostic toast (via `ActorCommand::ShowToast`)
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
        app.send_cmd(ActorCommand::ShowToast {
            message: "open_interest: malformed filter JSON".to_string(),
        });
        return;
    }

    app.send_cmd(ActorCommand::OpenInterest {
        filter_json,
        consumer_id,
        scope,
    });
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

    app.send_cmd(ActorCommand::CloseInterest {
        filter_json,
        consumer_id,
        scope,
    });
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

    app.send_cmd(ActorCommand::Kernel(nmp_core::KernelAction::OpenUri {
        uri,
    }));
}

/// Refcount a consumer's interest in `pubkey`'s kind:0 profile.
///
/// F-TTL — `force` (`c_int`, treated as `force != 0`) controls the lazy
/// re-verification gate for the cached profile. Pass `1` when the user
/// explicitly opened this author's profile screen or pulled to refresh;
/// pass `0` for background / `.onAppear` list-row claims. `force` is the
/// replacement for the removed `nmp_app_refresh_replaceable` symbol —
/// force-refresh is now an argument on the existing claim function, so no
/// new C-ABI symbol is added (keeps ffi-drift + surface-freeze green).
///
/// `liveness` (`c_int`) is the client freshness hint mapped to the registered
/// kind:0 interest's lifecycle:
/// * `0` = CacheOk — serve from cache; on a miss a OneShot fetch; no live sub.
///   Use for feed-row avatars.
/// * non-zero = Live — a Tailing kind:0 sub stays open while claimed so
///   profile edits arrive reactively. Use for an open profile screen.
/// Mixed claims on one pubkey resolve to Tailing (Live wins).
///
/// FFI-clean (D6): null/invalid pubkey is a silent no-op, never a panic.
#[no_mangle]
pub extern "C" fn nmp_app_claim_profile(
    app: *mut NmpApp,
    pubkey: *const c_char,
    consumer_id: *const c_char,
    force: c_int,
    liveness: c_int,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    app.send_cmd(ActorCommand::ClaimProfile {
        pubkey,
        consumer_id,
        force: force != 0,
        liveness: nmp_core::ProfileLiveness::from_ffi(liveness),
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_release_profile(
    app: *mut NmpApp,
    pubkey: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    app.send_cmd(ActorCommand::ReleaseProfile {
        pubkey,
        consumer_id,
    });
}

/// Claim an embedded event by `nostr:` URI (T180 / ADR-0034). Refcounted
/// per `consumer_id`; the kernel fetches the event over the OneshotApi
/// (single-writer interest registration — D4) when not yet in the store,
/// and surfaces it in snapshot `projections.claimed_events` keyed by
/// `primary_id` (event-id hex for `nevent`/`note`; `"kind:pubkey:d"` for
/// `naddr`). FFI-clean (D6): a null/invalid argument is a silent no-op,
/// never a panic. D8: forwards to the actor; no polling, no sync wait.
///
/// F-TTL — `force` (`c_int`, treated as `force != 0`) controls the lazy
/// re-verification gate. It only has an effect for `naddr` (addressable /
/// replaceable) URIs; for immutable `nevent`/`note` URIs it is a silent
/// no-op (those events carry no TTL record). Pass `1` when the user
/// explicitly navigated to / opened this article/event, or pulled to
/// refresh; pass `0` for background claims. Replaces the removed
/// `nmp_app_refresh_replaceable` symbol — no new C-ABI symbol is added.
#[no_mangle]
pub extern "C" fn nmp_app_claim_event(
    app: *mut NmpApp,
    uri: *const c_char,
    consumer_id: *const c_char,
    force: c_int,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };

    app.send_cmd(ActorCommand::ClaimEvent {
        uri,
        consumer_id,
        force: force != 0,
    });
}

/// Release a previously-claimed embedded event (T180 / ADR-0034). Mirrors
/// `nmp_app_release_profile`: decrements the per-consumer refcount in the
/// kernel's `event_claims` table; the kernel drops the row when the set
/// is empty. The OneshotApi interest itself is released EOSE-driven via
/// the existing `complete_unknown_oneshot` path. FFI-clean (D6): a null
/// or invalid argument is a silent no-op. D8: forwards to the actor.
#[no_mangle]
pub extern "C" fn nmp_app_release_event(
    app: *mut NmpApp,
    uri: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };

    app.send_cmd(ActorCommand::ReleaseEvent { uri, consumer_id });
}

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
            Some(k) if !nmp_nip18::is_repost_kind(k) => {
                set.insert(k);
            }
            None => return None,    // first invalid element → bail
            Some(_) => return None, // repost wrappers are derived, not primary
        }
    }
    Some(set)
}

/// ADR-0042 amendment (2026-06-12) — open the contact-feed declaration.
///
/// `primary_kinds_json` is a JSON array of unsigned 32-bit integers identifying
/// the primary content kinds the app wants to render, e.g. `"[1]"` for Chirp.
/// Repost wrappers are derived here (`6` for primary kind `1`, `16` for every
/// non-kind-1 primary target) before the compiled acquisition set is sent to
/// `nmp-core`. An empty array `"[]"` is a legitimate clear — same effect as
/// `nmp_app_close_contact_feed`. A malformed or non-array value, or any element
/// that is not a non-negative integer fitting in u32, surfaces a diagnostic
/// toast rather than a panic or silent registration (D6).
///
/// D8: fire-and-forget; the actor processes the command asynchronously.
#[no_mangle]
pub extern "C" fn nmp_app_open_contact_feed(app: *mut NmpApp, primary_kinds_json: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(kinds_str) = c_string_argument(primary_kinds_json) else {
        return;
    };

    let primary_kinds = match parse_primary_kinds_json(&kinds_str) {
        Some(set) => set,
        None => {
            app.send_cmd(nmp_core::ActorCommand::ShowToast {
                message: "open_contact_feed: malformed primary kinds JSON".to_string(),
            });
            return;
        }
    };
    let kinds = match nmp_nip18::try_acquisition_kinds_for_primary(primary_kinds) {
        Ok(kinds) => kinds,
        Err(_) => {
            app.send_cmd(nmp_core::ActorCommand::ShowToast {
                message: "open_contact_feed: primary kinds must not include repost wrappers"
                    .to_string(),
            });
            return;
        }
    };

    app.send_cmd(nmp_core::ActorCommand::OpenContactFeed { kinds });
}

/// ADR-0042 amendment (2026-06-12) — close the contact-feed subscription.
///
/// Withdraws all follow-feed M2 interests from the lifecycle registry;
/// `drain_lifecycle_tick` emits CLOSE frames for any live REQs on the next
/// idle tick. D6: a null `app` is a silent no-op.
///
/// D8: fire-and-forget; the actor processes the command asynchronously.
#[no_mangle]
pub extern "C" fn nmp_app_close_contact_feed(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(nmp_core::ActorCommand::CloseContactFeed);
}

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
    fn non_array_top_level_is_rejected() {
        assert!(parse_primary_kinds_json(r#"{"kinds":[1,6]}"#).is_none());
        assert!(parse_primary_kinds_json("1").is_none());
        assert!(parse_primary_kinds_json("null").is_none());
    }

    #[test]
    fn primary_kind_1_expands_to_kind_6_acquisition() {
        let primary = parse_primary_kinds_json("[1]").unwrap();
        assert_eq!(
            nmp_nip18::acquisition_kinds_for_primary(primary),
            BTreeSet::from([1u32, 6u32])
        );
    }

    #[test]
    fn non_kind_1_primary_expands_to_kind_16_acquisition() {
        let primary = parse_primary_kinds_json("[20]").unwrap();
        assert_eq!(
            nmp_nip18::acquisition_kinds_for_primary(primary),
            BTreeSet::from([16u32, 20u32])
        );
    }
}
