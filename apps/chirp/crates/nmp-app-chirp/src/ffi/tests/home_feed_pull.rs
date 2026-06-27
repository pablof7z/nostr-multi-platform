//! Gate: Chirp's home feed (`nmp.feed.home`) automatically adopts the
//! seq-ordered pull pager (ADR-0058 §8 6B) end-to-end through the **real**
//! Chirp composition seam.
//!
//! This is the home-feed sibling of
//! `interest_feed::tests::author_feed_load_older_grows_visible_window_past_first_page`
//! (which gates the per-open author feed). The home feed differs in one
//! load-bearing way: its `PullFeedController` reads a LIVE, fail-closed
//! active-follows `InterestShape` derived from the **active account** slot +
//! follow set (`register_op_feed_defaults` step 5a). So the gate proves the
//! whole chain:
//!
//!   `app.load_older_feed("nmp.feed.home")`  (the real seam,
//!        `nmp_app_load_older_feed` calls this verbatim)
//!     → `FeedRegistry::load_older`
//!       → `PullFeedController` (registered by `register_op_feed_defaults`)
//!         → live `ClosureInterestShape` provider (active account + follows)
//!           → `FeedPullPager::drain` over `app.feed_pull_fn()`
//!             → `pull_page_over` over the real `MemEventStore` (seq cursor)
//!               → `OpFeedEngine::on_kernel_event` (the apply closure)
//!                 → `grow_visible_window` (the advance closure)
//!                   → the `nmp.feed.home` projection GROWS.
//!
//! ## Why this is falsifiable (not a vacuous green)
//!
//! The late-old event (high seq, OLD `created_at`) is seeded ONLY into the
//! pull store — it is never pushed into the engine before `load_older`. The
//! engine therefore cannot surface it unless the seam→pager wiring actually
//! drains it from the store and ingests it. If ANY link breaks — the
//! controller is not registered under `"nmp.feed.home"`, the provider fails
//! closed for a signed-in account, the pager never drains, the apply closure
//! does not ingest, or the advance closure does not grow the window — the
//! visible projection does NOT grow past the first page and does NOT contain
//! the late-old event, so the two load-bearing assertions fail. Verified by
//! sabotage: forcing the step-5a provider to return `None` makes this test RED
//! while the no-account negative case stays green.
//!
//! A `created_at` cursor (the ADR-0058 §1 bug) would silently skip the
//! late-old event because its timestamp sorts below the first page; the seq
//! cursor sees it because seq is arrival-monotonic.

use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_ffi::{nmp_app_free, nmp_app_new, NmpApp};
use nmp_nip01::op_feed::{decode_op_feed_snapshot, OP_FEED_SNAPSHOT_KEY};
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

use super::super::ChirpHandle;
use super::helpers::register_app;

const ALICE: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const RELAY: &str = "wss://seed.example/";

/// One page (`DEFAULT_FEED_WINDOW_LIMIT`) plus enough extra roots that the
/// first snapshot is provably capped (`has_more`) and a single drain cannot
/// reveal everything in one go.
const RECENT_COUNT: usize = 100;
/// The very old `created_at` for the late-ingested event. Far below every
/// recent timestamp, so a `created_at` cursor would skip it.
const LATE_OLD_TS: u64 = 5;
const RECENT_TS_BASE: u64 = 2_000;

fn recent_id(i: usize) -> String {
    format!("{i:064x}")
}

fn late_old_id() -> String {
    "ee".repeat(32)
}

/// Build a kind:1 standalone-note `KernelEvent` authored by ALICE (a root in
/// the OP-feed engine, regardless of follow gating).
fn kernel_note(id: &str, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: ALICE.to_string(),
        kind: 1,
        created_at,
        tags: Vec::new(),
        content: format!("note {id}"),
        relay_provenance: Vec::new(),
    }
}

/// Insert the matching event into the real pull store with a monotonic seq.
fn store_note(store: &MemEventStore, id: &str, created_at: u64) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: ALICE.to_string(),
        created_at,
        kind: 1,
        tags: Vec::new(),
        content: format!("note {id}"),
        sig: "a".repeat(128),
    };
    store
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &RELAY.to_string(),
            1_000,
        )
        .expect("store insert must succeed");
}

/// Install a custom pull store into the app's shared event-store slot — the
/// same slot `app.feed_pull_fn()` (captured by `register_op_feed_defaults`)
/// reads on every `load_older`. Mirrors `interest_feed::tests::install_store`.
fn install_store(app_ref: &NmpApp, store: Arc<MemEventStore>) {
    *app_ref.event_store_handle().lock().unwrap() = Some(store);
}

/// Sign in `pubkey` by writing the live active-account slot the home-feed pull
/// provider reads (`register_op_feed_defaults` reads `active_account_handle`).
/// This is the same `Arc<Mutex<Option<String>>>` the actor would write on a
/// real sign-in; the provider self-includes the active account in `authors`,
/// so its own notes are covered without a kind:3.
fn sign_in(app_ref: &NmpApp, pubkey: &str) {
    *app_ref.active_account_handle().lock().unwrap() = Some(pubkey.to_string());
}

/// Read the decoded `nmp.feed.home` typed projection card ids — the exact
/// output the seam emits to a render shell. `None` when the key is absent or
/// cleared.
fn home_projection_ids(app_ref: &NmpApp) -> Option<Vec<String>> {
    let projections = app_ref.run_typed_snapshot_projections();
    let entry = projections
        .iter()
        .find(|p| p.key == OP_FEED_SNAPSHOT_KEY && !p.payload.is_empty())?;
    let snapshot = decode_op_feed_snapshot(&entry.payload).ok()?;
    Some(snapshot.cards.iter().map(|c| c.card.id.clone()).collect())
}

/// `has_more` from the live engine window the projection encodes.
fn home_has_more(handle: *mut ChirpHandle) -> bool {
    let snap = unsafe { &*handle }.snapshot();
    snap.page.map(|p| p.has_more).unwrap_or(false)
}

/// Seed the engine's first page directly through its `ObservedProjectionSink`
/// ingest path — the SAME path the live relay fan-out (and the pager's apply
/// closure) uses. This stands in for the live-push history a home feed
/// accumulates before the user scrolls; it does NOT touch the seam under test.
fn seed_engine_first_page(handle: *mut ChirpHandle, count: usize) {
    let engine = &unsafe { &*handle }.engine;
    for i in 0..count {
        engine.on_kernel_event(&kernel_note(&recent_id(i), RECENT_TS_BASE + i as u64));
    }
}

#[test]
fn chirp_home_load_older_engages_pull_with_active_account() {
    let app = nmp_app_new();
    assert!(!app.is_null());
    // REAL Chirp composition: `register_app` → `nmp_app_chirp_register` →
    // `nmp_native_runtime::register_op_feed_defaults(app, viewer, vec![1])`. The
    // `"nmp.feed.home"` PullFeedController is wired here, not synthesized.
    let handle = register_app(app);
    let app_ref: &NmpApp = unsafe { &*app };

    // ACTIVE signed-in account: the home-feed pull provider proves a live
    // account FIRST and self-includes it in `authors`.
    sign_in(app_ref, ALICE);

    // Seed the pull store with MORE THAN ONE PAGE of ALICE's notes, plus one
    // late-ingested event with an OLD `created_at` (highest seq, lowest ts).
    let store = Arc::new(MemEventStore::new());
    for i in 0..RECENT_COUNT {
        store_note(&store, &recent_id(i), RECENT_TS_BASE + i as u64);
    }
    store_note(&store, &late_old_id(), LATE_OLD_TS); // seq = RECENT_COUNT+1, ts = 5
    install_store(app_ref, store);

    // The engine's live-push history is exactly the recent page set — the
    // late-old event is ONLY in the pull store, never pushed. So if the seam
    // does not drain it, it can never appear.
    seed_engine_first_page(handle, RECENT_COUNT);

    // ── FIRST snapshot: capped at one page, has_more ─────────────────────────
    let first = home_projection_ids(app_ref).expect("nmp.feed.home projection present");
    assert_eq!(
        first.len(),
        nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        "first snapshot is capped at one page before load_older"
    );
    assert!(
        home_has_more(handle),
        "more roots than the page cap ⇒ has_more is true"
    );
    assert!(
        !first.contains(&late_old_id()),
        "the late-old event is NOT visible before load_older (it is pull-only)"
    );

    // ── Call the REAL seam: load_older_feed("nmp.feed.home") ─────────────────
    // `nmp_app_load_older_feed` (the C-ABI) calls this verbatim. A user
    // scrolling pages repeatedly; loop the on-demand drain until the seq pager
    // is exhausted (bounded so a broken seam cannot spin).
    let mut drains = 0usize;
    let mut progressed = false;
    while app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY) {
        progressed = true;
        drains += 1;
        assert!(
            drains < 16,
            "pager must exhaust in a bounded number of drains"
        );
    }
    assert!(
        progressed,
        "load_older must engage the pull pager at least once"
    );

    // ── GROW + late-old INCLUDED ─────────────────────────────────────────────
    let grown = home_projection_ids(app_ref).expect("nmp.feed.home projection after load_older");
    assert!(
        grown.len() > first.len(),
        "the visible nmp.feed.home projection GREW past the first page \
         (first={}, grown={})",
        first.len(),
        grown.len()
    );
    assert_eq!(
        grown.len(),
        RECENT_COUNT + 1,
        "every distinct root — the full recent page set plus the pulled \
         late-old event — is now visible"
    );
    assert!(
        grown.contains(&late_old_id()),
        "the late-old event (high seq, OLD created_at) is now visible — the seq \
         pull cursor drained it from the store and ingested it; a created_at \
         cursor would have skipped it (ADR-0058 §1)"
    );

    // Sanity: the late-old event really does sort below the entire first page,
    // so its inclusion is only explicable by the seq cursor.
    assert!(
        LATE_OLD_TS < RECENT_TS_BASE,
        "late-old created_at is below the first-page window"
    );

    nmp_app_free(app);
}

#[test]
fn chirp_home_load_older_without_active_account_is_safe_noop() {
    let app = nmp_app_new();
    assert!(!app.is_null());
    let handle = register_app(app);
    let app_ref: &NmpApp = unsafe { &*app };

    // NO active account: the slot is left as `None` (never signed in).

    // A small live-push baseline + a pull store that DOES contain the late-old
    // event. With no account the provider must fail closed regardless.
    let baseline = 5usize;
    let store = Arc::new(MemEventStore::new());
    for i in 0..baseline {
        store_note(&store, &recent_id(i), RECENT_TS_BASE + i as u64);
    }
    store_note(&store, &late_old_id(), LATE_OLD_TS);
    install_store(app_ref, store);
    seed_engine_first_page(handle, baseline);

    let before = home_projection_ids(app_ref).expect("nmp.feed.home projection present");
    assert_eq!(before.len(), baseline);

    // The real seam must be a safe no-op (fail closed), never a crash.
    let changed = app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY);
    assert!(
        !changed,
        "no active account ⇒ provider returns None ⇒ load_older fails closed (no pull)"
    );

    let after = home_projection_ids(app_ref).expect("nmp.feed.home projection still present");
    assert_eq!(
        after.len(),
        before.len(),
        "fail-closed load_older did not grow the window"
    );
    assert!(
        !after.contains(&late_old_id()),
        "fail-closed load_older did NOT pull the late-old event into the feed"
    );

    nmp_app_free(app);
}
