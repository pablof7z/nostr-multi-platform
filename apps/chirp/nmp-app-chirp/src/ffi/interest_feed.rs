//! Chirp per-open author / thread feed registration (M2, ADR-0042 §5.1,
//! BACKLOG V-112).
//!
//! These symbols replace the old `author_view` / `thread_view` snapshot
//! projections (and the four `open_author` / `open_thread` / `close_author` /
//! `close_thread` C-ABI symbols + their bespoke kernel machinery) that Step D
//! of the V-112 handoff deletes. A profile screen and a thread screen each
//! render a **flat** list of notes — every matching kind:1/6 as its own
//! top-level row — which the OP-centric home feed engine structurally cannot
//! express (it rolls a followed author's replies up as *attribution* under
//! other people's roots). [`nmp_nip01::FlatFeed`] is that flat machine; this
//! module is its host-side composition root.
//!
//! ## What one open does
//!
//! `nmp_app_chirp_open_author_feed(app, pubkey_hex)` performs the two halves
//! the read path needs, with the `{1,6}` note-kind policy defined ONCE here so
//! the two halves can never diverge:
//!
//! 1. **Kernel admission** — pushes a generic `open_interest`
//!    (`{"kinds":[1,6],"authors":[pk]}`, consumer `author-<pk>`, scope Global)
//!    through the existing [`nmp_ffi::nmp_app_open_interest`] so the kernel
//!    stores matching events into `self.events` (a non-followed author would
//!    otherwise be dropped before storage — V-112 gating fact #1) and fans
//!    them out to every [`nmp_core::KernelEventObserver`].
//! 2. **Feed render** — constructs a [`nmp_nip01::FlatFeed`] over the same
//!    `{1,6}` author predicate and registers it as BOTH a feed controller
//!    (output, under `nmp.feed.author.<pk>`) AND a kernel event observer
//!    (ingest) through [`NmpApp::register_feed_with_observer`], which tracks
//!    the observer id under the key for teardown.
//!
//! `nmp_app_chirp_close_author_feed(app, pubkey_hex)` reverses both halves:
//! [`NmpApp::unregister_feed`] drops the controller + snapshot projection +
//! observer, and a matching `close_interest` detaches the kernel
//! subscription. The thread variants are identical with a root-id-keyed
//! predicate (`nmp.feed.thread.<id>`, consumer `thread-<id>`).
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never names `nmp-nip01`/`FlatFeed`; this app crate is
//!   the composition point (the same role `register_op_feed_defaults` plays
//!   for the home feed). The `{1,6}` kind decision lives here, never in the
//!   substrate.
//! * **D6** — every entry point is fire-and-forget. Null pointers, invalid
//!   UTF-8, and poisoned mutexes degrade silently rather than raising across
//!   the FFI.

use std::ffi::c_char;

use nmp_ffi::{nmp_app_close_interest, nmp_app_open_interest, NmpApp};
use nmp_nip01::{author_feed_predicate, thread_feed_predicate, FlatFeed};

use super::helpers::c_string_opt;

/// The note-kind policy for both the author and thread flat feeds: kind:1
/// (text note) + kind:6 (repost). Defined ONCE so the `open_interest` filter
/// (kernel admission) and the `FlatFeed` predicate (render gate) always agree —
/// a divergence would either admit events the feed silently drops or starve the
/// feed of events the kernel never stored.
const FEED_KINDS: [u32; 2] = [1, 6];

/// Scope passed to `open_interest`: `1` = Global (account-agnostic). A visited
/// profile / open thread is NOT re-routed on account switch — it pins a
/// concrete author / root id, not the active account.
const SCOPE_GLOBAL: u32 = 1;

/// `nmp.feed.author.<pubkey_hex>` — the snapshot key ProfileView reads.
#[must_use]
fn author_feed_key(pubkey_hex: &str) -> String {
    format!("nmp.feed.author.{pubkey_hex}")
}

/// `nmp.feed.thread.<event_id_hex>` — the snapshot key ThreadScreen reads.
#[must_use]
fn thread_feed_key(event_id_hex: &str) -> String {
    format!("nmp.feed.thread.{event_id_hex}")
}

/// Build the `open_interest` filter JSON for [`FEED_KINDS`] over one tag
/// dimension (`"authors"` or `"#e"`). Hand-built (not `serde_json`) because the
/// shape is fixed and tiny; the value is re-parsed kernel-side into an
/// `InterestShape` whose hash gives deterministic dedup.
#[must_use]
fn feed_filter_json(dimension: &str, value: &str) -> String {
    format!(r#"{{"kinds":[1,6],"{dimension}":["{value}"]}}"#)
}

/// Open the flat author feed for `pubkey_hex`.
///
/// Registers a [`FlatFeed`] under `nmp.feed.author.<pubkey_hex>` (read by
/// `ProfileView`) and pushes the kernel interest that admits the author's
/// kind:1/6 into storage. Idempotent at the registry level: a re-open of the
/// same author replaces the controller and revokes the prior observer (see
/// [`NmpApp::register_feed_with_observer`]); the kernel `open_interest`
/// refcounts the `author-<pk>` consumer.
///
/// D6 — a null `app` or non-UTF-8 `pubkey_hex` is a silent no-op.
///
/// `app` MUST outlive the feed; call `nmp_app_chirp_close_author_feed` (or rely
/// on the `nmp_app_free` actor join) before freeing it.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_open_author_feed(app: *mut NmpApp, pubkey_hex: *const c_char) {
    if app.is_null() {
        return;
    }
    let Some(pubkey) = c_string_opt(pubkey_hex).filter(|s| !s.is_empty()) else {
        return;
    };
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`,
    // live for the duration of this call. The reference is not held past return
    // (the registered observer/controller hold their own `Arc`s, not `&app`).
    let app_ref = unsafe { &*app };

    let feed = FlatFeed::new(author_feed_predicate(pubkey.clone(), FEED_KINDS.to_vec()));
    app_ref.register_feed_with_observer(author_feed_key(&pubkey), feed.clone(), feed);

    open_interest_for(
        app,
        &feed_filter_json("authors", &pubkey),
        &author_consumer(&pubkey),
    );
}

/// Close the flat author feed for `pubkey_hex`: tear down the feed registration
/// (controller + snapshot projection + ingest observer) and detach the kernel
/// interest. Idempotent — a close of an unopened author is a harmless no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_close_author_feed(app: *mut NmpApp, pubkey_hex: *const c_char) {
    if app.is_null() {
        return;
    }
    let Some(pubkey) = c_string_opt(pubkey_hex).filter(|s| !s.is_empty()) else {
        return;
    };
    // SAFETY: see `nmp_app_chirp_open_author_feed`.
    let app_ref = unsafe { &*app };

    let _ = app_ref.unregister_feed(&author_feed_key(&pubkey));
    close_interest_for(
        app,
        &feed_filter_json("authors", &pubkey),
        &author_consumer(&pubkey),
    );
}

/// Open the flat thread feed for `event_id_hex` (the thread root).
///
/// Registers a [`FlatFeed`] under `nmp.feed.thread.<event_id_hex>` (read by
/// `ThreadScreen`) whose predicate admits the root by id AND every kind:1/6
/// that references it via an `#e` tag, and pushes the kernel interest that
/// admits those `#e` referrers into storage. (The root itself arrives through
/// whatever interest opened the screen that linked here — the predicate admits
/// it by id when it is already stored, and the `#e` interest pulls the
/// replies.)
///
/// D6 — a null `app` or non-UTF-8 `event_id_hex` is a silent no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_open_thread_feed(app: *mut NmpApp, event_id_hex: *const c_char) {
    if app.is_null() {
        return;
    }
    let Some(root_id) = c_string_opt(event_id_hex).filter(|s| !s.is_empty()) else {
        return;
    };
    // SAFETY: see `nmp_app_chirp_open_author_feed`.
    let app_ref = unsafe { &*app };

    let feed = FlatFeed::new(thread_feed_predicate(root_id.clone(), FEED_KINDS.to_vec()));
    app_ref.register_feed_with_observer(thread_feed_key(&root_id), feed.clone(), feed);

    open_interest_for(
        app,
        &feed_filter_json("#e", &root_id),
        &thread_consumer(&root_id),
    );
}

/// Close the flat thread feed for `event_id_hex`: tear down the feed
/// registration and detach the kernel interest. Idempotent.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_close_thread_feed(app: *mut NmpApp, event_id_hex: *const c_char) {
    if app.is_null() {
        return;
    }
    let Some(root_id) = c_string_opt(event_id_hex).filter(|s| !s.is_empty()) else {
        return;
    };
    // SAFETY: see `nmp_app_chirp_open_author_feed`.
    let app_ref = unsafe { &*app };

    let _ = app_ref.unregister_feed(&thread_feed_key(&root_id));
    close_interest_for(
        app,
        &feed_filter_json("#e", &root_id),
        &thread_consumer(&root_id),
    );
}

/// Refcount-owner key for an author interest. Stable per author so a re-open
/// shares the live subscription and the matching close detaches the same slot.
#[must_use]
fn author_consumer(pubkey_hex: &str) -> String {
    format!("author-{pubkey_hex}")
}

/// Refcount-owner key for a thread interest.
#[must_use]
fn thread_consumer(root_id_hex: &str) -> String {
    format!("thread-{root_id_hex}")
}

/// Push a kernel `open_interest` through the existing validated C-ABI seam
/// (reuses its malformed-filter rejection + `InterestShape` dedup). The
/// filter/consumer strings are short-lived `CString`s passed by pointer for the
/// duration of the call.
fn open_interest_for(app: *mut NmpApp, filter_json: &str, consumer_id: &str) {
    let (Ok(filter), Ok(consumer)) = (
        std::ffi::CString::new(filter_json),
        std::ffi::CString::new(consumer_id),
    ) else {
        return;
    };
    nmp_app_open_interest(app, filter.as_ptr(), consumer.as_ptr(), SCOPE_GLOBAL);
}

/// Push a kernel `close_interest` mirroring [`open_interest_for`].
fn close_interest_for(app: *mut NmpApp, filter_json: &str, consumer_id: &str) {
    let (Ok(filter), Ok(consumer)) = (
        std::ffi::CString::new(filter_json),
        std::ffi::CString::new(consumer_id),
    ) else {
        return;
    };
    nmp_app_close_interest(app, filter.as_ptr(), consumer.as_ptr(), SCOPE_GLOBAL);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_namespaced_per_consumer() {
        assert_eq!(author_feed_key("abc"), "nmp.feed.author.abc");
        assert_eq!(thread_feed_key("def"), "nmp.feed.thread.def");
        assert_eq!(author_consumer("abc"), "author-abc");
        assert_eq!(thread_consumer("def"), "thread-def");
    }

    #[test]
    fn filter_json_carries_the_feed_kinds_and_dimension() {
        // The {1,6} policy in the filter MUST match FEED_KINDS (the predicate
        // source), or the kernel admits events the feed drops / vice versa.
        assert_eq!(FEED_KINDS, [1, 6]);
        assert_eq!(
            feed_filter_json("authors", "abc"),
            r#"{"kinds":[1,6],"authors":["abc"]}"#
        );
        // `r##"…"##` — the inner `"#e"` contains a `"#` sequence that would
        // terminate a single-hash raw string early.
        assert_eq!(
            feed_filter_json("#e", "root1"),
            r##"{"kinds":[1,6],"#e":["root1"]}"##
        );
    }

    #[test]
    fn feed_filter_json_parses_as_a_valid_interest_shape() {
        // Guards the hand-built JSON against the kernel-side parser the open
        // path feeds it into — a malformed filter would be silently rejected.
        for json in [
            feed_filter_json("authors", "abc"),
            feed_filter_json("#e", "root1"),
        ] {
            assert!(
                nmp_core::planner::InterestShape::from_filter_json(&json).is_some(),
                "filter must parse: {json}"
            );
        }
    }
}
