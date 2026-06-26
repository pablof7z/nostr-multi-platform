//! Cold-start regression test for issue #1643.
//!
//! **The bug**: on cold start, a user's kind:10003 bookmark list already in the
//! local store was never surfaced to the `ObservedProjectionSink` fan-out because
//! nothing pushed a demand interest (`authors=[pubkey]` + `kinds=[10003]`).
//!
//! **The fix**: `BookmarksRuntimeController` pushes `active_bookmark_list_interest`
//! on sign-in, and `register_bookmark_runtime` registers the projection observer
//! BEFORE the first tick so the synchronous cache-serve drain (triggered by the
//! interest push inside the actor's `EnsureInterest` handler) fires AFTER the
//! observer is already installed.
//!
//! # What this test proves
//!
//! A kind:10003 event stored in the local event store reaches a `ObservedProjectionSink`
//! via the cache-serve drain when an `authors=[pubkey] / kinds=[10003]` interest is
//! registered — WITHOUT any relay delivery.  It also proves that if the observer is
//! registered AFTER the interest, the observer receives nothing (the ordering
//! contract `register_bookmark_runtime` enforces at lines 50/67 of bookmarks_runtime.rs
//! is load-bearing).
//!
//! The test lives in nmp-core (not nmp-nip51 or nmp-defaults) because the
//! critical kernel APIs — `inject_replaceable_event`, `register_interest`,
//! `clear_served_interest_shapes`, and `set_event_observers_handle` — are all
//! `pub(crate)` within this crate.  Using a `CapturingObserver` (instead of the
//! production `BookmarkListProjection`) is intentional: the
//! `ObservedProjectionSink::on_kernel_event` call is the SAME path both use, so
//! proving the observer fires proves the projection would fire.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::actor::{new_event_observer_slot, register_rust_observer, ObservedProjectionSink};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::substrate::KernelEvent;

use super::*;

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
/// A bookmark-list event-id tag: `["e", <event-id>]`.
const BOOKMARKED_EVENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
/// A synthetic kind:10003 event id.
const KIND10003_EVENT_ID: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const KIND_BOOKMARK_LIST: u32 = 10_003;

/// Capturing observer: counts all kind:10003 events it receives and records
/// the `["e", <id>]` payload tags so the cold-start test can assert on shape.
struct CapturingBookmarkObserver {
    /// Number of kind:10003 `on_kernel_event` calls received.
    count: AtomicU32,
    /// Tags from the LAST kind:10003 event received.
    last_tags: Mutex<Vec<Vec<String>>>,
}

impl CapturingBookmarkObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            count: AtomicU32::new(0),
            last_tags: Mutex::new(Vec::new()),
        })
    }

    fn received_count(&self) -> u32 {
        self.count.load(Ordering::SeqCst)
    }

    fn last_tags(&self) -> Vec<Vec<String>> {
        self.last_tags
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl ObservedProjectionSink for CapturingBookmarkObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_BOOKMARK_LIST {
            return;
        }
        self.count.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut guard) = self.last_tags.lock() {
            *guard = event.tags.clone();
        }
    }
}

/// Seed a kind:10003 event for ALICE into the kernel store (relay path).
///
/// This represents a bookmark list that arrived from the relay in a prior session
/// and was persisted to the local store.  On restart the in-memory state is lost
/// but the store still has the event.
fn seed_kind10003(kernel: &mut Kernel) {
    kernel
        .inject_replaceable_event(
            KIND10003_EVENT_ID,
            ALICE,
            1_700_000_000,
            KIND_BOOKMARK_LIST,
            // One bookmark: `["e", BOOKMARKED_EVENT]`
            vec![vec!["e".to_string(), BOOKMARKED_EVENT.to_string()]],
            "wss://relay.example/",
            1_700_000_000_000,
        )
        .expect("seeding kind:10003 must succeed");
}

/// Register a kind:10003 interest for ALICE — the same shape
/// `active_bookmark_list_interest(ALICE)` produces.
fn push_bookmark_interest(kernel: &mut Kernel) {
    let shape = crate::planner::InterestShape {
        authors: std::collections::BTreeSet::from([ALICE.to_string()]),
        kinds: std::collections::BTreeSet::from([KIND_BOOKMARK_LIST]),
        ..Default::default()
    };
    kernel.register_interest(
        &[crate::kernel::cache_serve::InterestRegistration {
            identity: crate::subs::SubIdentity::new(
                crate::subs::SubOwnerKey::new("test-bookmark-cold-start"),
                crate::subs::SubKey::new("bookmark-cold-start"),
                crate::subs::SubScope::Global,
            ),
            interest: crate::planner::LogicalInterest {
                shape,
                ..Default::default()
            },
            policy: crate::kernel::cache_serve::InterestWrite::Replace,
        }],
        "test-bookmark-cold-start",
    );
}

/// # Cold-start regression test (#1643)
///
/// Proves that a kind:10003 event stored in the local store is delivered to a
/// `ObservedProjectionSink` via the cache-serve drain when an `authors=[active_pubkey]
/// / kinds=[10003]` interest is registered, WITHOUT any relay delivery.
///
/// **Observer-before-push ordering (the load-bearing contract)**:
///
/// The fix registers the `BookmarkListProjection` observer BEFORE calling the first
/// tick (which pushes the interest).  This test exercises the same ordering:
/// observer registered FIRST → interest registered SECOND → cache-serve fires →
/// observer receives the stored kind:10003.
///
/// **NON-VACUITY**: if you swap the order (interest first, observer second), the
/// test `observer_after_interest_push_receives_nothing_from_cache_serve` below
/// proves the observer gets zero events — which is exactly the pre-fix bug.
#[test]
fn stored_kind10003_reaches_observer_via_cache_serve_drain() {
    // ── Phase 1: seed the event store "from a prior session" ─────────────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());
    seed_kind10003(&mut kernel);

    // Precondition: seeding routes the event through `project_accepted_event`
    // and `notify_event_observers`, but there is no observer yet — that's
    // expected (warm path, not the cold-start path we're testing).

    // ── Phase 2: simulate cold restart — clear in-memory cache-serve state ────
    //
    // In production the process restarts (losing all RAM state); here we evict
    // the served-interest completion set so the next `register_interest` call
    // re-runs the cache-serve drain, mirroring a fresh kernel that has never
    // served this interest shape.
    kernel.clear_served_interest_shapes();

    // ── Phase 3: OBSERVER FIRST — ordering contract from bookmarks_runtime.rs ─
    //
    // `register_bookmark_runtime` line 51 registers the projection observer;
    // line 67 registers the tick observer (which pushes the interest on the first
    // tick).  Observer registration MUST precede the interest push so the
    // cache-serve drain that fires synchronously inside `register_interest`
    // reaches the observer.
    let slot = new_event_observer_slot();
    let observer = CapturingBookmarkObserver::new();
    register_rust_observer(&slot, observer.clone());
    kernel.set_event_observers_handle(slot);

    // ── Phase 4: INTEREST PUSH (the tick's EnsureInterest → register_interest) ─
    //
    // In production the actor handles `InterestsCommand::EnsureInterest` by calling
    // `kernel.register_interest(...)` with the interest shape.  We call it
    // directly here so the test does not need the actor thread.  The cache-serve
    // drain runs synchronously inside `register_interest`, delivering the stored
    // kind:10003 to the observer.
    push_bookmark_interest(&mut kernel);

    // ── Phase 5: assert — event surfaces WITHOUT relay delivery ───────────────
    assert_eq!(
        observer.received_count(),
        1,
        "the stored kind:10003 must reach the observer via cache-serve WITHOUT any \
         relay delivery — this is the #1643 cold-start regression gate; \
         received_count = 0 means the observer-before-push ordering is broken or \
         the interest was not pushed"
    );

    let tags = observer.last_tags();
    let has_bookmarked_event = tags
        .iter()
        .any(|tag| tag.len() >= 2 && tag[0] == "e" && tag[1] == BOOKMARKED_EVENT);
    assert!(
        has_bookmarked_event,
        "the cache-served kind:10003 must carry the stored bookmark item \
         ['e', '{BOOKMARKED_EVENT}']; got tags: {tags:?}"
    );
}

/// # Observer-after-push ordering regression gate
///
/// Proves that if the observer is registered AFTER the interest is pushed (the
/// pre-fix ordering), the observer receives ZERO events — because the cache-serve
/// drain completes BEFORE the observer exists.
///
/// This test must go RED if the production code changes to defer the cache-serve
/// drain (which would accidentally restore the pre-fix bug in the wrong direction).
/// It is the counter-example that gives `stored_kind10003_reaches_observer_via_cache_serve_drain`
/// its NON-VACUITY: if the ordering didn't matter, both tests would trivially pass.
#[test]
fn observer_after_interest_push_receives_nothing_from_cache_serve() {
    // ── Seed + cold restart (same as above) ───────────────────────────────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());
    seed_kind10003(&mut kernel);
    kernel.clear_served_interest_shapes();

    // ── WRONG order: interest FIRST ───────────────────────────────────────────
    //
    // No observer is installed yet.  The cache-serve drain runs now and
    // delivers the stored kind:10003 to… nobody.
    push_bookmark_interest(&mut kernel);

    // ── Observer registered AFTER the drain already completed ─────────────────
    let slot = new_event_observer_slot();
    let late_observer = CapturingBookmarkObserver::new();
    register_rust_observer(&slot, late_observer.clone());
    kernel.set_event_observers_handle(slot);

    // The late observer missed the cache-serve drain — it gets zero events.
    assert_eq!(
        late_observer.received_count(),
        0,
        "an observer registered AFTER the interest push must NOT receive \
         retrospective cache-serve events — this is the pre-fix state; if this \
         assertion fails it means the cache-serve drain is no longer synchronous \
         or the ordering contract has been loosened, which would break \
         the cold-start fix from the opposite direction"
    );
}
