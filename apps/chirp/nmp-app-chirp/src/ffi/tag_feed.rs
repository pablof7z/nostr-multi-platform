//! Chirp hashtag feed FFI.
//!
//! The shell dispatches only "open this tag" intent. This module owns the
//! NIP-12 tag normalization, primary note declaration, global interest scope,
//! compiled acquisition filter shape, and stable consumer id.
//!
//! Structurally identical to the author / thread flat feeds
//! ([`super::interest_feed`]): one open performs the two halves the read path
//! needs — a kernel interest (admission) and a [`FlatFeed`] render projection
//! under `nmp.feed.tag.<tag>` (read by `HashtagFeedView`) — wired through the
//! ADR-0062 `open_observed_interest` catch-up path so a freshly opened tag feed
//! replays whatever the kernel read-cache already holds before live relay
//! pushes fill it. The `{1}` primary kind decision lives here; NIP-18 derives
//! the `{1,6}` acquisition set (D0 — `nmp-core` never owns that policy).

use std::ffi::c_char;
use std::sync::Arc;

use nmp_core::KernelEventObserver;
use nmp_feed::{ClosureInterestShape, PullFeedController};
use nmp_ffi::NmpApp;
use nmp_nip01::{tag_feed_predicate, tag_feed_shape, FlatFeed};
use nmp_planner::InterestShape;

use super::helpers::{c_string_opt, make_pull_fn};
use super::interest_feed::{
    close_interest_for, feed_acquisition_kinds, register_typed_feed_sidecar, FEED_PRIMARY_KINDS,
};

/// Scope passed to the interest: `1` = Global (account-agnostic). A hashtag
/// feed is not re-routed on account switch; it pins a public Nostr tag.
const SCOPE_GLOBAL: u32 = 1;

/// `nmp.feed.tag.<tag>` — the snapshot key `HashtagFeedView` reads.
#[must_use]
fn tag_feed_key(tag: &str) -> String {
    format!("nmp.feed.tag.{tag}")
}

/// Refcount-owner key for a tag interest. Stable per tag so a re-open shares the
/// live subscription and the matching close detaches the same slot.
#[must_use]
fn tag_consumer(tag: &str) -> String {
    format!("tag-{tag}")
}

#[must_use]
fn normalize_tag(value: &str) -> Option<String> {
    let tag = value.trim().trim_start_matches('#').to_lowercase();
    (!tag.is_empty()).then_some(tag)
}

/// The wire / replay filter for the tag feed: `{"kinds":[1,6],"#t":[tag]}`. The
/// `[1,6]` set is the NIP-18 acquisition derivation of Chirp's primary `[1]`
/// declaration, so kernel admission and the render predicate cannot diverge.
#[must_use]
fn tag_feed_filter_json(tag: &str) -> Option<String> {
    let kinds = nmp_nip18::try_acquisition_kinds_for_primary(FEED_PRIMARY_KINDS)
        .ok()?
        .into_iter()
        .collect::<Vec<_>>();
    Some(serde_json::json!({ "kinds": kinds, "#t": [tag] }).to_string())
}

/// Open a global hashtag feed for primary kind:1 notes carrying the normalized
/// NIP-12 `#t` tag (NIP-18 repost wrappers derived from that declaration).
///
/// Registers a [`FlatFeed`] under `nmp.feed.tag.<tag>` (read by
/// `HashtagFeedView`) AND pushes the kernel interest that admits the tag's
/// kind:1/6 into storage, with ADR-0062 catch-up replay of the read-cache.
/// Idempotent: a re-open replaces the controller and refcounts the `tag-<tag>`
/// consumer.
///
/// D6 — a null `app` or empty/non-UTF-8 `tag` is a silent no-op.
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
    // live for the duration of this call. The reference is not held past return
    // (the registered observer/controller hold their own `Arc`s, not `&app`).
    let app_ref = unsafe { &*app };

    let Some(acquisition_kinds) = feed_acquisition_kinds() else {
        return;
    };
    let feed = FlatFeed::new(tag_feed_predicate(tag.clone(), acquisition_kinds.clone()));
    let key = tag_feed_key(&tag);
    // Drain store history by ingest seq via PullFeedController (FlatFeed = push
    // observer). The pull shape is the covered `{kinds, #t:[tag]}` `Ttag` shape.
    let tag_for_shape = tag.clone();
    let kinds_for_shape = acquisition_kinds.clone();
    let provider = Arc::new(ClosureInterestShape::new(move || {
        Some(tag_feed_shape(tag_for_shape.clone(), kinds_for_shape.clone()))
    }));
    let pull = make_pull_fn(app_ref.event_store_handle());
    let apply: nmp_feed::FeedApply = {
        let f = feed.clone();
        Arc::new(move |ev| {
            let before = f.len();
            KernelEventObserver::on_kernel_event(&*f, ev);
            f.len() > before
        })
    };
    // Viewport grow after each drained page — reveals the newly-pulled older
    // rows in the emitted sidecar (viewport-only, no second pull).
    let advance: nmp_feed::FeedAdvance = {
        let f = feed.clone();
        Arc::new(move || {
            f.grow_visible_window();
        })
    };
    let pull_ctrl = PullFeedController::new(provider, pull, apply, advance);
    // register_feed_with_observer returns the muted observer id (ADR-0062).
    let observer_id = app_ref.register_feed_with_observer(key.clone(), pull_ctrl, feed.clone());
    register_typed_feed_sidecar(app_ref, key, feed);

    // ADR-0062 tag replay: one shape covering `{"kinds":[1,6],"#t":[tag]}`.
    let Some(filter_json) = tag_feed_filter_json(&tag) else {
        return;
    };
    let replay_shapes: Vec<InterestShape> = InterestShape::from_filter_json(&filter_json)
        .map(|s| vec![s])
        .unwrap_or_default();
    app_ref.open_observed_interest(
        &filter_json,
        &tag_consumer(&tag),
        SCOPE_GLOBAL,
        observer_id,
        replay_shapes,
        nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
    );
}

/// Close the flat hashtag feed for `tag`: tear down the feed registration
/// (controller + snapshot projection + ingest observer) and detach the kernel
/// interest. Idempotent — a close of an unopened tag is a harmless no-op.
///
/// D6 — a null `app` or empty/non-UTF-8 `tag` is a silent no-op.
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

    let _ = app_ref.unregister_feed(&tag_feed_key(&tag));
    if let Some(filter_json) = tag_feed_filter_json(&tag) {
        close_interest_for(app, &filter_json, &tag_consumer(&tag));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_consumer_is_stable_and_namespaced() {
        assert_eq!(tag_consumer("nostr"), "tag-nostr");
    }

    #[test]
    fn tag_feed_key_is_namespaced() {
        assert_eq!(tag_feed_key("nostr"), "nmp.feed.tag.nostr");
    }

    #[test]
    fn tag_filter_json_carries_chirp_tag_policy() {
        assert_eq!(
            tag_feed_filter_json("nostr").unwrap(),
            r##"{"kinds":[1,6],"#t":["nostr"]}"##
        );
    }

    #[test]
    fn tag_filter_json_parses_as_interest_shape() {
        let json = tag_feed_filter_json("nostr").unwrap();
        assert!(
            nmp_planner::InterestShape::from_filter_json(&json).is_some(),
            "filter must parse: {json}"
        );
    }

    #[test]
    fn tag_feed_normalizes_user_input_in_app_ffi_layer() {
        assert_eq!(normalize_tag("  #Nostr  "), Some("nostr".to_string()));
        assert_eq!(normalize_tag("nostr"), Some("nostr".to_string()));
        assert_eq!(normalize_tag("###"), None);
    }
}
