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
//! 1. **Kernel interest** — pushes a generic `open_interest`
//!    (`{"kinds":[1,6],"authors":[pk]}`, consumer `author-<pk>`, scope Global)
//!    through the existing [`nmp_ffi::nmp_app_open_interest`] so the kernel
//!    subscribes for matching relay events and fans accepted stored events out
//!    to every [`nmp_core::KernelEventObserver`].
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
use std::sync::Arc;

use nmp_core::store::{EventStore, StoreQuery, StoredEvent};
use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_feed::{ClosureInterestShape, PullFeedController};
use nmp_ffi::{
    nmp_app_close_contact_feed, nmp_app_close_interest, nmp_app_open_contact_feed,
    nmp_app_open_interest, NmpApp,
};
use nmp_nip01::op_feed::{
    encode_op_feed_snapshot, OP_FEED_FILE_IDENTIFIER, OP_FEED_SCHEMA_ID, OP_FEED_SCHEMA_VERSION,
};
use nmp_nip01::{author_feed_predicate, thread_feed_predicate, FlatFeed};

use super::helpers::{author_feed_shape, c_string_opt, make_pull_fn, thread_feed_shape};

/// The note-kind policy for both the author and thread flat feeds: kind:1
/// (text note) + kind:6 (repost). Defined ONCE so the `open_interest` filter
/// (kernel admission) and the `FlatFeed` predicate (render gate) always agree —
/// a divergence would either admit events the feed silently drops or starve the
/// feed of events the kernel never stored.
const FEED_KINDS: [u32; 2] = [1, 6];

/// Bound store seeding so an old account with a large author history cannot
/// make a screen open unbounded. Live relay pushes continue filling the feed.
const FEED_SEED_LIMIT: usize = 512;

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
        let snapshot = feed.snapshot(&nmp_feed::FeedRequest::default());
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
    seed_author_feed_from_store(app_ref, &feed, &pubkey);
    let key = author_feed_key(&pubkey);
    // B1: drain store history by ingest seq via PullFeedController (FlatFeed = push observer).
    // PullFeedController::new always succeeds; load_older fails closed if the
    // provider returns None (which cannot happen here — pubkey is always valid).
    let pk_for_shape = pubkey.clone();
    let provider = Arc::new(ClosureInterestShape::new(move || author_feed_shape(&pk_for_shape, &FEED_KINDS)));
    let pull = make_pull_fn(app_ref.event_store_handle());
    let apply: nmp_feed::FeedApply = { let f = feed.clone(); Arc::new(move |ev| KernelEventObserver::on_kernel_event(&*f, ev)) };
    let advance: nmp_feed::FeedAdvance = Arc::new(|| ());
    let pull_ctrl = PullFeedController::new(provider, pull, apply, advance);
    app_ref.register_feed_with_observer(key.clone(), pull_ctrl, feed.clone());
    register_typed_feed_sidecar(app_ref, key, feed);

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
    seed_thread_feed_from_store(app_ref, &feed, &root_id);
    let key = thread_feed_key(&root_id);
    // B1: pull the reply tail (#e-covered shape); root-by-id seeded above.
    // PullFeedController::new always succeeds; load_older fails closed if the
    // provider returns None (which cannot happen here — root_id is always valid).
    let root_for_shape = root_id.clone();
    let provider = Arc::new(ClosureInterestShape::new(move || thread_feed_shape(&root_for_shape, &FEED_KINDS)));
    let pull = make_pull_fn(app_ref.event_store_handle());
    let apply: nmp_feed::FeedApply = { let f = feed.clone(); Arc::new(move |ev| KernelEventObserver::on_kernel_event(&*f, ev)) };
    let advance: nmp_feed::FeedAdvance = Arc::new(|| ());
    let pull_ctrl = PullFeedController::new(provider, pull, apply, advance);
    app_ref.register_feed_with_observer(key.clone(), pull_ctrl, feed.clone());
    register_typed_feed_sidecar(app_ref, key, feed);

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

/// The home-feed kind policy for Chirp's contact-feed subscription: kind:1
/// (text note) + kind:6 (repost). Defined ONCE here — the single source of
/// truth for the `{1, 6}` decision (D0: `nmp-core` no longer hardcodes it).
/// `nmp_app_chirp_open_home_feed` passes this via the generic
/// `nmp_app_open_contact_feed` verb; `nmp_app_chirp_close_home_feed` uses the
/// matching `nmp_app_close_contact_feed`. Any app wanting different kinds
/// calls the generic verbs directly with its own set.
///
/// `pub(crate)` so in-crate tests can assert the constant value without
/// duplicating the literal.
pub(crate) const HOME_FEED_KINDS_JSON: &str = "[1,6]";

/// Open Chirp's home (contact) feed — the subscription that REQs kind:1 and
/// kind:6 events from the active account's follow set.
///
/// Delegates to the generic `nmp_app_open_contact_feed` with
/// `HOME_FEED_KINDS_JSON = "[1,6]"` so the `{1, 6}` literal lives in EXACTLY
/// ONE place (this constant). App shells that previously called
/// `nmp_app_open_timeline` must call this instead (ADR-0042 amendment
/// 2026-06-12).
///
/// D6 — a null `app` is a silent no-op (forwarded by `nmp_app_open_contact_feed`).
#[no_mangle]
pub extern "C" fn nmp_app_chirp_open_home_feed(app: *mut NmpApp) {
    if let Ok(kinds_c) = std::ffi::CString::new(HOME_FEED_KINDS_JSON) {
        nmp_app_open_contact_feed(app, kinds_c.as_ptr());
    }
}

/// Close Chirp's home (contact) feed. Mirrors `nmp_app_chirp_open_home_feed`;
/// withdraws all follow-feed M2 interests and emits CLOSE frames.
///
/// D6 — a null `app` is a silent no-op.
#[no_mangle]
pub extern "C" fn nmp_app_chirp_close_home_feed(app: *mut NmpApp) {
    nmp_app_close_contact_feed(app);
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

fn seed_author_feed_from_store(app: &NmpApp, feed: &FlatFeed, pubkey_hex: &str) {
    let Some(author) = hex32(pubkey_hex) else {
        return;
    };
    let Some(store) = event_store(app) else {
        return;
    };
    seed_query(
        feed,
        &*store,
        StoreQuery::AuthorKind {
            author,
            kinds: FEED_KINDS.to_vec(),
            since: None,
            until: None,
        },
    );
}

fn seed_thread_feed_from_store(app: &NmpApp, feed: &FlatFeed, root_id_hex: &str) {
    if let Some(root) = app.event_by_id(root_id_hex) {
        KernelEventObserver::on_kernel_event(feed, &root);
    }
    let Some(target) = hex32(root_id_hex) else {
        return;
    };
    let Some(store) = event_store(app) else {
        return;
    };
    seed_query(
        feed,
        &*store,
        StoreQuery::Etag {
            target,
            kinds: FEED_KINDS.to_vec(),
        },
    );
}

fn seed_query(feed: &FlatFeed, store: &dyn EventStore, query: StoreQuery) {
    let Ok(events) = store.query(&query, FEED_SEED_LIMIT) else {
        return;
    };
    for stored in events {
        KernelEventObserver::on_kernel_event(feed, &kernel_event_from_stored(&stored, store));
    }
}

fn event_store(app: &NmpApp) -> Option<Arc<dyn EventStore>> {
    app.event_store_handle().lock().ok()?.clone()
}

fn kernel_event_from_stored(stored: &StoredEvent, store: &dyn EventStore) -> KernelEvent {
    KernelEvent {
        id: stored.raw.id.clone(),
        author: stored.raw.pubkey.clone(),
        created_at: stored.raw.created_at,
        kind: stored.raw.kind,
        tags: stored.raw.tags.clone(),
        content: stored.raw.content.clone(),
        relay_provenance: nmp_core::slots::relay_provenance_for_event(store, &stored.raw.id),
    }
}

fn hex32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (idx, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(chunk).ok()?;
        out[idx] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    use nmp_core::store::{MemEventStore, RawEvent, VerifiedEvent};
    use nmp_core::WireProjectionState;
    use nmp_ffi::{nmp_app_free, nmp_app_new};

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

    #[test]
    fn author_feed_open_seeds_cached_kind1_and_close_removes_projection() {
        let app = nmp_app_new();
        assert!(!app.is_null());
        let store = Arc::new(MemEventStore::new());
        let pubkey = "11".repeat(32);
        insert_raw(
            &store,
            RawEvent {
                id: "a1".repeat(32),
                pubkey: pubkey.clone(),
                created_at: 10,
                kind: 1,
                tags: vec![],
                content: "older".into(),
                sig: "a".repeat(128),
            },
        );
        insert_raw(
            &store,
            RawEvent {
                id: "a2".repeat(32),
                pubkey: pubkey.clone(),
                created_at: 20,
                kind: 1,
                tags: vec![],
                content: "newer".into(),
                sig: "a".repeat(128),
            },
        );
        install_store(app, store);

        let pubkey_c = CString::new(pubkey.clone()).unwrap();
        nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
        let ids = read_typed_card_ids(app, &author_feed_key(&pubkey))
            .expect("author feed projection present after open");
        assert_eq!(ids, vec!["a2".repeat(32), "a1".repeat(32)]);

        nmp_app_chirp_close_author_feed(app, pubkey_c.as_ptr());
        let gone = typed_projection_is_gone(app, &author_feed_key(&pubkey));
        assert!(gone, "author feed projection must be gone after close");
        nmp_app_free(app);
    }

    #[test]
    fn thread_feed_open_seeds_cached_root_and_replies() {
        let app = nmp_app_new();
        assert!(!app.is_null());
        let store = Arc::new(MemEventStore::new());
        let root_id = "b1".repeat(32);
        insert_raw(
            &store,
            RawEvent {
                id: root_id.clone(),
                pubkey: "22".repeat(32),
                created_at: 10,
                kind: 1,
                tags: vec![],
                content: "root".into(),
                sig: "a".repeat(128),
            },
        );
        insert_raw(
            &store,
            RawEvent {
                id: "b2".repeat(32),
                pubkey: "33".repeat(32),
                created_at: 20,
                kind: 1,
                tags: vec![vec!["e".into(), root_id.clone()]],
                content: "reply".into(),
                sig: "a".repeat(128),
            },
        );
        install_store(app, store);

        let root_c = CString::new(root_id.clone()).unwrap();
        nmp_app_chirp_open_thread_feed(app, root_c.as_ptr());
        let ids = read_typed_card_ids(app, &thread_feed_key(&root_id))
            .expect("thread feed projection present after open");
        assert_eq!(ids, vec!["b2".repeat(32), root_id.clone()]);

        nmp_app_chirp_close_thread_feed(app, root_c.as_ptr());
        let gone = typed_projection_is_gone(app, &thread_feed_key(&root_id));
        assert!(gone, "thread feed projection must be gone after close");
        nmp_app_free(app);
    }

    #[test]
    fn author_feed_open_emits_typed_op_feed_sidecar_and_close_removes_it() {
        let app = nmp_app_new();
        assert!(!app.is_null());
        let store = Arc::new(MemEventStore::new());
        let pubkey = "11".repeat(32);
        insert_raw(
            &store,
            RawEvent {
                id: "a1".repeat(32),
                pubkey: pubkey.clone(),
                created_at: 10,
                kind: 1,
                tags: vec![],
                content: "older".into(),
                sig: "a".repeat(128),
            },
        );
        insert_raw(
            &store,
            RawEvent {
                id: "a2".repeat(32),
                pubkey: pubkey.clone(),
                created_at: 20,
                kind: 1,
                tags: vec![],
                content: "newer".into(),
                sig: "a".repeat(128),
            },
        );
        install_store(app, store);

        let pubkey_c = CString::new(pubkey.clone()).unwrap();
        nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());

        let key = author_feed_key(&pubkey);
        let app_ref = unsafe { &*app };
        let typed = app_ref.run_typed_snapshot_projections();
        let entry = typed.iter().find(|p| p.key == key).expect("typed sidecar");

        assert_eq!(entry.schema_id, OP_FEED_SCHEMA_ID);
        assert_eq!(entry.schema_version, OP_FEED_SCHEMA_VERSION);
        assert_eq!(entry.file_identifier, "NOFS");

        let snapshot = nmp_nip01::op_feed::decode_op_feed_snapshot(&entry.payload)
            .expect("typed payload decodes as a NOFS op-feed snapshot");
        let ids: Vec<String> = snapshot.cards.iter().map(|c| c.card.id.clone()).collect();
        assert_eq!(ids, vec!["a2".repeat(32), "a1".repeat(32)]);

        nmp_app_chirp_close_author_feed(app, pubkey_c.as_ptr());
        let typed_after = app_ref.run_typed_snapshot_projections();
        let clear = typed_after
            .iter()
            .find(|p| p.key == key)
            .expect("Cleared row");
        assert_eq!(clear.state, WireProjectionState::Cleared);
        assert!(clear.payload.is_empty());
        let typed_again = app_ref.run_typed_snapshot_projections();
        assert!(
            typed_again.iter().all(|p| p.key != key),
            "typed Cleared row must be one-shot"
        );
        nmp_app_free(app);
    }

    fn install_store(app: *mut NmpApp, store: Arc<MemEventStore>) {
        let app_ref = unsafe { &*app };
        *app_ref.event_store_handle().lock().unwrap() = Some(store);
    }

    fn insert_raw(store: &MemEventStore, raw: RawEvent) {
        store
            .insert(
                VerifiedEvent::from_raw_unchecked(raw),
                &"wss://seed.example/".to_string(),
                1_000,
            )
            .unwrap();
    }

    /// Return the decoded op-feed card IDs for `key` via the typed sidecar lane,
    /// or `None` when the key is absent / cleared. Replaces the deleted generic
    /// JSON lane (rule A6).
    fn read_typed_card_ids(app: *mut NmpApp, key: &str) -> Option<Vec<String>> {
        let app_ref: &NmpApp = unsafe { &*app };
        let projections = app_ref.run_typed_snapshot_projections();
        let entry = projections.iter().find(|p| p.key == key && !p.payload.is_empty())?;
        let snapshot = nmp_nip01::op_feed::decode_op_feed_snapshot(&entry.payload).ok()?;
        Some(snapshot.cards.iter().map(|c| c.card.id.clone()).collect())
    }

    /// Return `true` when the typed sidecar for `key` is absent or cleared.
    fn typed_projection_is_gone(app: *mut NmpApp, key: &str) -> bool {
        let app_ref: &NmpApp = unsafe { &*app };
        let projections = app_ref.run_typed_snapshot_projections();
        projections.iter().all(|p| p.key != key || p.payload.is_empty())
    }
}
