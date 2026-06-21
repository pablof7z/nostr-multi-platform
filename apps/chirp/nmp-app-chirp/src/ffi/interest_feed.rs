//! Chirp per-open author / thread feed registration (M2, ADR-0042 §5.1,
//! BACKLOG V-112).
//!
//! These symbols replace the old `author_view` / `thread_view` snapshot
//! projections (and the four `open_author` / `open_thread` / `close_author` /
//! `close_thread` C-ABI symbols + their bespoke kernel machinery) that Step D
//! of the V-112 handoff deletes. A profile screen and a thread screen each
//! render a **flat** list of notes — every matching kind:1 plus derived
//! repost wrappers as its own
//! top-level row — which the OP-centric home feed engine structurally cannot
//! express (it rolls a followed author's replies up as *attribution* under
//! other people's roots). [`nmp_nip01::FlatFeed`] is that flat machine; this
//! module is its host-side composition root.
//!
//! ## What one open does
//!
//! `nmp_app_chirp_open_author_feed(app, pubkey_hex)` performs the two halves
//! the read path needs, with primary note kinds defined ONCE here and repost
//! wrapper acquisition derived by `nmp-nip18` so the two halves can never
//! diverge:
//!
//! 1. **Kernel interest** — derives acquisition kinds from Chirp's primary
//!    `[1]` declaration, then pushes a generic `open_interest`
//!    with the compiled acquisition kinds, consumer `author-<pk>`, scope
//!    Global, through the existing [`nmp_ffi::nmp_app_open_interest`] so the
//!    kernel subscribes for matching relay events and fans accepted stored
//!    events out to every [`nmp_core::KernelEventObserver`].
//! 2. **Feed render** — constructs a [`nmp_nip01::FlatFeed`] over the same
//!    compiled author predicate and registers it as BOTH a feed controller
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
//!   for the home feed). The primary kind decision lives here, never in the
//!   substrate, and wrapper kinds are derived below the app-facing declaration.
//! * **D6** — every entry point is fire-and-forget. Null pointers, invalid
//!   UTF-8, and poisoned mutexes degrade silently rather than raising across
//!   the FFI.

use std::ffi::c_char;
use std::sync::Arc;

use nmp_core::KernelEventObserver;
use nmp_feed::{ClosureInterestShape, PullFeedController};
use nmp_ffi::{
    clear_active_follows_feed, declare_active_follows_feed, nmp_app_close_interest, NmpApp,
};
use nmp_nip01::op_feed::{
    encode_op_feed_snapshot, OP_FEED_FILE_IDENTIFIER, OP_FEED_SCHEMA_ID, OP_FEED_SCHEMA_VERSION,
};
use nmp_nip01::{author_feed_predicate, thread_feed_predicate, FlatFeed};

use super::helpers::{author_feed_shape, c_string_opt, make_pull_fn, thread_feed_shape};

/// Chirp's primary flat-feed content kind: kind:1 short-text notes.
///
/// Repost wrappers are not app-declared primary content. They are derived via
/// `nmp_nip18::acquisition_kinds_for_primary` so the `open_interest` filter
/// (kernel admission) and the `FlatFeed` predicate (render gate) always agree.
pub(crate) const FEED_PRIMARY_KINDS: [u32; 1] = [1];

fn feed_acquisition_kinds() -> Option<Vec<u32>> {
    nmp_nip18::try_acquisition_kinds_for_primary(FEED_PRIMARY_KINDS)
        .ok()
        .map(|kinds| kinds.into_iter().collect())
}

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

/// Register the typed NOFS op-feed sidecar alongside the generic `Value`
/// projection that `register_feed_with_observer` already installed. Uses the
/// same `encode_op_feed_snapshot` wire shape as the home feed (no new schema).
/// Teardown via `unregister_feed` covers both lanes — no extra step needed.
fn register_typed_feed_sidecar(app: &NmpApp, key: String, feed: Arc<FlatFeed>) {
    app.register_typed_snapshot_projection(key.clone(), move || {
        // Emit the CURRENT viewport, including rows revealed by prior
        // `load_older` drains (the `advance` closure grows it). A fixed
        // `FeedRequest::default()` would cap the sidecar at the first page, so
        // pulled older rows would ingest but never become user-visible.
        let snapshot = feed.snapshot_current_window();
        Some(nmp_core::TypedProjectionData {
            key: key.clone(),
            schema_id: OP_FEED_SCHEMA_ID.to_string(),
            schema_version: OP_FEED_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(OP_FEED_FILE_IDENTIFIER).into_owned(),
            payload: encode_op_feed_snapshot(&snapshot),
            ..Default::default()
        })
    });
}

/// Build the `open_interest` filter JSON for derived acquisition kinds over one tag
/// dimension (`"authors"` or `"#e"`). Hand-built (not `serde_json`) because the
/// shape is fixed and tiny; the value is re-parsed kernel-side into an
/// `InterestShape` whose hash gives deterministic dedup.
#[must_use]
fn feed_filter_json(dimension: &str, value: &str) -> Option<String> {
    let kinds = feed_acquisition_kinds()?
        .into_iter()
        .map(|k| k.to_string())
        .collect::<Vec<_>>()
        .join(",");
    Some(format!(
        r#"{{"kinds":[{kinds}],"{dimension}":["{value}"]}}"#
    ))
}

/// Open the flat author feed for `pubkey_hex`.
///
/// Registers a [`FlatFeed`] under `nmp.feed.author.<pubkey_hex>` (read by
/// `ProfileView`) and pushes the kernel interest that admits the author's
/// primary kind:1 plus derived kind:6 reposts into storage. Idempotent at the
/// registry level: a re-open of the same author replaces the controller and
/// revokes the prior observer (see [`NmpApp::register_feed_with_observer`]);
/// the kernel `open_interest` refcounts the `author-<pk>` consumer.
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

    let Some(acquisition_kinds) = feed_acquisition_kinds() else {
        return;
    };
    let feed = FlatFeed::new(author_feed_predicate(
        pubkey.clone(),
        acquisition_kinds.clone(),
    ));
    // ADR-0062: no store seeding here; the kernel owns catch-up via
    // open_observed_interest (register_feed_with_observer → muted observer
    // → OpenObservedInterest command replays self.events → activate).
    let key = author_feed_key(&pubkey);
    // B1: drain store history by ingest seq via PullFeedController (FlatFeed = push observer).
    // PullFeedController::new always succeeds; load_older fails closed if the
    // provider returns None (which cannot happen here — pubkey is always valid).
    let pk_for_shape = pubkey.clone();
    let kinds_for_shape = acquisition_kinds.clone();
    let provider = Arc::new(ClosureInterestShape::new(move || {
        author_feed_shape(&pk_for_shape, &kinds_for_shape)
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
    // After a drained page is ingested, grow the render viewport so the newly-
    // pulled older rows become user-visible in the emitted sidecar (they sort
    // below the first page). Viewport-only — no second pull.
    let advance: nmp_feed::FeedAdvance = {
        let f = feed.clone();
        Arc::new(move || {
            f.grow_visible_window();
        })
    };
    let pull_ctrl = PullFeedController::new(provider, pull, apply, advance);
    // register_feed_with_observer now returns the muted observer id (ADR-0062).
    let observer_id = app_ref.register_feed_with_observer(key.clone(), pull_ctrl, feed.clone());
    register_typed_feed_sidecar(app_ref, key, feed);

    // ADR-0062 author replay: one shape covering `{"authors":[pk],"kinds":[1,6]}`.
    let replay_shapes: Vec<nmp_planner::InterestShape> = {
        let pk_str = pubkey.clone();
        let k = acquisition_kinds
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let json = format!(r#"{{"kinds":[{k}],"authors":["{pk_str}"]}}"#);
        nmp_planner::InterestShape::from_filter_json(&json)
            .map(|s| vec![s])
            .unwrap_or_default()
    };
    if let Some(filter_json) = feed_filter_json("authors", &pubkey) {
        app_ref.open_observed_interest(
            &filter_json,
            &author_consumer(&pubkey),
            SCOPE_GLOBAL,
            observer_id,
            replay_shapes,
            nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        );
    }
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
    if let Some(filter_json) = feed_filter_json("authors", &pubkey) {
        close_interest_for(app, &filter_json, &author_consumer(&pubkey));
    }
}

/// Open the flat thread feed for `event_id_hex` (the thread root).
///
/// Registers a [`FlatFeed`] under `nmp.feed.thread.<event_id_hex>` (read by
/// `ThreadScreen`) whose predicate admits the root by id AND every admitted
/// primary kind:1 event or derived kind:6 repost wrapper that references it
/// via an `#e` tag, and pushes the kernel interest that admits those `#e`
/// referrers into storage. (The root itself arrives through whatever interest
/// opened the screen that linked here — the predicate admits it by id when it
/// is already stored, and the `#e` interest pulls the replies.)
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

    let Some(acquisition_kinds) = feed_acquisition_kinds() else {
        return;
    };
    let feed = FlatFeed::new(thread_feed_predicate(
        root_id.clone(),
        acquisition_kinds.clone(),
    ));
    // ADR-0062: no store seeding here; the kernel owns catch-up via
    // open_observed_interest. Two shapes cover the thread: `#e` replies +
    // root-by-id (the root itself may already be in the read-cache).
    let key = thread_feed_key(&root_id);
    // B1: pull the reply tail (#e-covered shape); root-by-id replayed below.
    // PullFeedController::new always succeeds; load_older fails closed if the
    // provider returns None (which cannot happen here — root_id is always valid).
    let root_for_shape = root_id.clone();
    let kinds_for_shape = acquisition_kinds.clone();
    let provider = Arc::new(ClosureInterestShape::new(move || {
        thread_feed_shape(&root_for_shape, &kinds_for_shape)
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
    // Viewport grow after each drained page — reveals the newly-pulled reply
    // tail in the emitted sidecar (viewport-only, no second pull).
    let advance: nmp_feed::FeedAdvance = {
        let f = feed.clone();
        Arc::new(move || {
            f.grow_visible_window();
        })
    };
    let pull_ctrl = PullFeedController::new(provider, pull, apply, advance);
    // register_feed_with_observer now returns the muted observer id (ADR-0062).
    let observer_id = app_ref.register_feed_with_observer(key.clone(), pull_ctrl, feed.clone());
    register_typed_feed_sidecar(app_ref, key, feed);

    // ADR-0062 thread replay: two shapes — `#e` replies and root-by-id.
    let replay_shapes: Vec<nmp_planner::InterestShape> = {
        let k = acquisition_kinds
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let etag_json = format!(r##"{{"kinds":[{k}],"#e":["{root_id}"]}}"##);
        let id_json = format!(r#"{{"ids":["{root_id}"]}}"#);
        let mut shapes = Vec::new();
        if let Some(s) = nmp_planner::InterestShape::from_filter_json(&etag_json) {
            shapes.push(s);
        }
        if let Some(s) = nmp_planner::InterestShape::from_filter_json(&id_json) {
            shapes.push(s);
        }
        shapes
    };
    if let Some(filter_json) = feed_filter_json("#e", &root_id) {
        app_ref.open_observed_interest(
            &filter_json,
            &thread_consumer(&root_id),
            SCOPE_GLOBAL,
            observer_id,
            replay_shapes,
            nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        );
    }
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
    if let Some(filter_json) = feed_filter_json("#e", &root_id) {
        close_interest_for(app, &filter_json, &thread_consumer(&root_id));
    }
}

/// Chirp's home-feed primary content kind declaration: kind:1 text notes.
///
/// Repost wrappers are derived below this app-facing declaration; Chirp does
/// not enumerate kind:6 as primary feed policy.
///
/// `pub(crate)` so in-crate tests can assert the constant value without
/// duplicating the literal.
pub(crate) const HOME_FEED_PRIMARY_KINDS_JSON: &str = "[1]";

/// Declare Chirp's home feed from the active account's current follow set.
///
/// Delegates to the Rust declared-feed helper with
/// `HOME_FEED_PRIMARY_KINDS_JSON = "[1]"`. App shells that previously called
/// `nmp_app_open_timeline` must call this instead.
///
/// D6 — a null `app` is a silent no-op.
#[no_mangle]
pub extern "C" fn nmp_app_chirp_open_home_feed(app: *mut NmpApp) {
    if let Ok(kinds_c) = std::ffi::CString::new(HOME_FEED_PRIMARY_KINDS_JSON) {
        declare_active_follows_feed(app, kinds_c.as_ptr());
    }
}

/// Clear Chirp's active-follows home-feed declaration.
///
/// D6 — a null `app` is a silent no-op.
#[no_mangle]
pub extern "C" fn nmp_app_chirp_close_home_feed(app: *mut NmpApp) {
    clear_active_follows_feed(app);
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

/// Push a kernel `close_interest` for an interest opened via
/// `open_observed_interest`.
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
#[path = "interest_feed/tests.rs"]
mod tests;
