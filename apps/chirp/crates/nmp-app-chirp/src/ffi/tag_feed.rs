//! Chirp hashtag feed FFI.
//!
//! The shell dispatches only "open this tag" intent. This module owns the
//! NIP-12 tag normalization, the PRIMARY note-kind declaration, and the typed
//! feed-session lifecycle.
//!
//! ## #1740 step 6 — migrated to the typed `open_feed` session API
//!
//! The open path no longer hand-builds a `{"kinds":[1,6],"#t":[…]}` filter JSON
//! and pushes a raw `nmp_app_open_interest`. It declares only Chirp's PRIMARY
//! content kind (`[1]`) inside a typed [`FeedParams`] with
//! [`FeedScope::Tag`] acquisition and lets [`NmpApp::open_feed`] drive the
//! perspective compiler ([`nmp_native_runtime::compile_feed_params`]):
//!
//! * wrapper kinds 6/16 are DERIVED below the app boundary by the compiler (the
//!   app never names `[1,6]`), so the `#t` acquisition interest the kernel opens
//!   is identical to the one the raw path opened — but the kind policy now lives
//!   in ONE place (the compiler), not duplicated in this app crate;
//! * the session ALSO registers an event-aware `#t` render engine + typed
//!   sidecar under the `nmp.feed.tag.<tag>` projection key (the raw path opened a
//!   bare interest with no render — this is additive: a tag screen can now read a
//!   real feed snapshot, matching the author/thread feeds);
//! * teardown is HANDLE-based ([`NmpApp::close_feed`]) — close never re-derives a
//!   filter (D4).
//!
//! Fire-and-forget shells keep the same one-arg open/close C-ABI shape. Because
//! the open is fire-and-forget (no handle returned to the shell), the minted
//! [`nmp_feed::FeedHandle`] is parked in a process-local tag→handle map so the
//! matching `nmp_app_chirp_close_tag_feed(tag)` can close the right session
//! without the shell threading a handle through (the same key-based affordance
//! the raw `tag-<tag>` consumer id provided, now over a typed handle).
//!
//! Doctrine: D0 — the PRIMARY-kind decision lives here; wrapper derivation and
//! the `#t` predicate live below the app boundary (the compiler). D6 — a null
//! `app`, non-UTF-8 / empty tag, or a fail-closed compile is a silent no-op.
//!
//! ## Known limitation — no `#t` pull back-fill (not a regression)
//!
//! The session engine acquires via `OpenInterest` (not the ADR-0062
//! muted-observer replay the author/thread feeds use), so the render feed shows
//! LIVE matching events. The catch-up path for already-cached events is the pull
//! pager — but the pull substrate has no `#t` `StoreQuery`
//! (`nmp-core` `cache_serve/queries.rs` covers `#e`/`#p`/author+kind/KindTime
//! only), so a `#t`-only shape is `UnsupportedInterestShape` and a note cached
//! BEFORE the open is not back-filled. This is a bounded late-joiner gap, not a
//! regression: the raw `open_interest` path this migration replaced rendered
//! NOTHING. A future `#t` store query lights up back-fill with no app change
//! here (pinned by `tests::tag_feed::pre_cached_tag_notes_are_not_pull_backfilled`).

use std::collections::HashMap;
use std::ffi::c_char;
use std::sync::Mutex;

use nmp_feed::{
    FeedAdmission, FeedHandle, FeedParams, FeedRanking, FeedScope, FeedWindow, ProjectionKey,
    TagTerm, DEFAULT_FEED_WINDOW_LIMIT,
};
use nmp_ffi::{FeedOpenError, NmpApp};

use super::helpers::c_string_opt;

const CHIRP_FEED_PRIMARY_KINDS: [u32; 1] = [1];

/// `nmp.feed.tag.<tag>` — the snapshot key a tag screen reads (additive render).
#[must_use]
fn tag_feed_key(tag: &str) -> String {
    format!("nmp.feed.tag.{tag}")
}

/// Identity of the `NmpApp` a parked tag handle belongs to.
///
/// A [`nmp_feed::FeedHandle`]'s `session_id` is scoped to ONE `NmpApp`'s session
/// registry; closing it against a DIFFERENT app would address an unrelated
/// same-id session. So parked handles are keyed by the owning app's pointer
/// address (stable for the app's lifetime) AND the tag, never by tag alone. This
/// keeps multiple live apps' tag sessions disjoint (D8 — no cross-app clobber).
type AppId = usize;

#[must_use]
fn app_id(app: *mut NmpApp) -> AppId {
    app as AppId
}

/// Process-local map of the currently-open tag sessions, keyed by the OWNING app
/// and tag, so the fire-and-forget `close_tag_feed(app, tag)` looks up the typed
/// handle the matching open on THAT app minted.
///
/// A re-open of an already-open `(app, tag)` closes the prior session BEFORE the
/// replacement opens (see [`nmp_app_chirp_open_tag_feed`]). A poisoned lock
/// degrades to "no open tags tracked" — a soft fail (the kernel interest is still
/// refcounted; a leak is bounded by app lifetime), never a panic across the FFI
/// (D6).
static OPEN_TAGS: Mutex<Option<HashMap<(AppId, String), FeedHandle>>> = Mutex::new(None);

/// Park the handle the latest open on `app` minted for `tag`.
///
/// The open path closes any PRIOR session for `(app, tag)` (via [`forget_tag`])
/// BEFORE opening the replacement, so the map never holds a stale handle when
/// this runs — a plain insert. (Closing the prior session AFTER the replacement
/// registered would be unsound: both sessions share the `nmp.feed.tag.<tag>`
/// projection key, so the old session's key-based teardown would unregister the
/// NEW feed's controller/projection.)
fn remember_tag(app: *mut NmpApp, tag: &str, handle: FeedHandle) {
    if let Ok(mut guard) = OPEN_TAGS.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert((app_id(app), tag.to_string()), handle);
    }
}

fn forget_tag(app: *mut NmpApp, tag: &str) -> Option<FeedHandle> {
    OPEN_TAGS.lock().ok().and_then(|mut guard| {
        guard
            .as_mut()
            .and_then(|map| map.remove(&(app_id(app), tag.to_string())))
    })
}

/// Drop ALL parked tag handles owned by `app`. Called from
/// `nmp_app_chirp_unregister` so a freed app leaves no stale entries behind — a
/// later app reusing the same pointer address can never collide with a dangling
/// session id (the freed app's sessions are gone with it). The handles are not
/// `close_feed`'d here: the app is being torn down (its session registry is
/// dropped with it), so dropping the parked handles is the whole job.
pub(super) fn forget_app_tags(app: *mut NmpApp) {
    if let Ok(mut guard) = OPEN_TAGS.lock() {
        if let Some(map) = guard.as_mut() {
            let id = app_id(app);
            map.retain(|(owner, _), _| *owner != id);
        }
    }
}

#[must_use]
fn normalize_tag(value: &str) -> Option<String> {
    let tag = value.trim().trim_start_matches('#').to_lowercase();
    (!tag.is_empty()).then_some(tag)
}

/// The typed feed declaration for a hashtag feed: PRIMARY kind:1 with
/// [`FeedScope::Tag`] acquisition. Chirp declares ONLY its primary content kind;
/// the compiler derives NIP-18 wrapper acquisition (`6`/`16`) below this
/// boundary, so `[1,6]` never appears in app-facing code.
#[must_use]
fn tag_feed_params(tag: &str) -> FeedParams {
    FeedParams {
        primary_kinds: CHIRP_FEED_PRIMARY_KINDS.to_vec(),
        render: nmp_feed::FeedRender::OpCentric,
        acquisition: FeedScope::Tag {
            term: TagTerm(tag.to_string()),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow {
            initial_limit: DEFAULT_FEED_WINDOW_LIMIT,
        },
        projection: ProjectionKey(tag_feed_key(tag)),
    }
}

/// The `open_feed` compiler adapter — the SAME `compile_feed_params` path the
/// op-feed session tests drive. `open_feed` validates the primary kinds and
/// derives wrapper acquisition (`kinds`) below the app boundary before this runs.
/// A `Tag` scope compiles to the `#t` acquisition interest + an event-aware `#t`
/// render engine.
fn compiler(
    app: &NmpApp,
    params: &FeedParams,
    kinds: &std::collections::BTreeSet<u32>,
) -> Result<nmp_feed::FeedSessionBuild, FeedOpenError> {
    nmp_native_runtime::compile_feed_params(app, params, kinds)
}

/// Open a global hashtag feed for primary kind:1 notes carrying the normalized
/// NIP-12 `#t` tag, with NIP-18 repost wrapper acquisition derived below the app
/// boundary by the perspective compiler.
///
/// Migrated to [`NmpApp::open_feed`] (#1740 step 6): no raw `open_interest`, no
/// app-built `[1,6]` filter JSON. Fire-and-forget — a null `app`, empty/non-UTF-8
/// tag, or fail-closed compile is a silent no-op (D6). The minted handle is
/// parked for `nmp_app_chirp_close_tag_feed`.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_open_tag_feed(app: *mut NmpApp, tag: *const c_char) {
    if app.is_null() {
        return;
    }
    let Some(tag) = c_string_opt(tag).and_then(|value| normalize_tag(&value)) else {
        return;
    };
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`,
    // live for the duration of this call. `open_feed` holds its own `Arc`s, not
    // a borrow of `app`.
    let app_ref = unsafe { &*app };

    // A re-open of the same tag mints a NEW session under the SAME projection
    // key. Tear the PRIOR session down FIRST (its key-based teardown must run
    // before the replacement registers, or it would remove the new feed's
    // controller/projection), THEN open + park the replacement. Net effect: at
    // most one live session per tag, and the surviving session is always the
    // latest (no leak, no clobber).
    if let Some(prior) = forget_tag(app, &tag) {
        let _ = app_ref.close_feed(&prior);
    }
    if let Ok(handle) = app_ref.open_feed(&tag_feed_params(&tag), &compiler) {
        remember_tag(app, &tag, handle);
    }
}

/// Close a hashtag feed opened by [`nmp_app_chirp_open_tag_feed`]: tear the typed
/// session down via its handle (controller + render + typed sidecar + the `#t`
/// acquisition interest), idempotently. A close of an unopened tag is a harmless
/// no-op (D6).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_close_tag_feed(app: *mut NmpApp, tag: *const c_char) {
    if app.is_null() {
        return;
    }
    let Some(tag) = c_string_opt(tag).and_then(|value| normalize_tag(&value)) else {
        return;
    };
    // SAFETY: see `nmp_app_chirp_open_tag_feed`.
    let app_ref = unsafe { &*app };

    if let Some(handle) = forget_tag(app, &tag) {
        let _ = app_ref.close_feed(&handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_feed_normalizes_user_input_in_app_ffi_layer() {
        assert_eq!(normalize_tag("  #Nostr  "), Some("nostr".to_string()));
        assert_eq!(normalize_tag("nostr"), Some("nostr".to_string()));
        assert_eq!(normalize_tag("###"), None);
    }

    #[test]
    fn tag_feed_key_is_stable_and_namespaced() {
        assert_eq!(tag_feed_key("nostr"), "nmp.feed.tag.nostr");
    }

    #[test]
    fn tag_feed_params_declare_primary_only_no_wrapper_leak() {
        // D0 — the app declares ONLY its PRIMARY content kind. Wrapper kinds
        // (6/16) are DERIVED by the compiler below this boundary; they must NOT
        // appear in the app-declared params (no `[1,6]` policy leak).
        let params = tag_feed_params("nostr");
        assert_eq!(
            params.primary_kinds,
            vec![1],
            "tag feed declares primary kind:1 only — wrapper derivation is the compiler's job"
        );
        assert!(
            !params.primary_kinds.contains(&6) && !params.primary_kinds.contains(&16),
            "no wrapper kind leaks into the app-facing primary declaration"
        );
        assert!(
            matches!(params.acquisition, FeedScope::Tag { term: TagTerm(ref t) } if t == "nostr"),
            "typed Tag acquisition over the normalized term"
        );
        // The declared primaries pass fail-closed validation and compile to the
        // SAME acquisition kind set the raw path opened (1 ∪ derived wrappers ∪ 5).
        // Validation lives in the composition layer (`nmp_ffi`), not in the
        // protocol-agnostic `nmp-feed` engine.
        let kinds =
            nmp_ffi::validate_feed_params(&params).expect("primary [1] is a valid declaration");
        assert!(kinds.contains(&1), "primary kind:1 acquired");
        assert!(
            kinds.contains(&6),
            "NIP-18 wrapper kind:6 derived by the compiler"
        );
    }
}
