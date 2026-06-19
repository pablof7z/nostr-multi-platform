//! Real-feed end-to-end test — ADR-0058 §1 fix (B3).
//!
//! Proves that a `PullFeedController` wired to `pull_page_over` over a real
//! `MemEventStore` picks up an event with a LOW `created_at` that was ingested
//! LATE (high seq), even though its timestamp falls below the display cursor of
//! the already-rendered window.
//!
//! A `created_at`-based cursor would silently skip this event (§1 bug).
//! A seq-based drain (the ADR-0058 fix) sees it because seq is arrival-monotonic:
//! no matter how old the event's timestamp is, its store seq is always after every
//! earlier ingest.
//!
//! ## What is wired
//!
//! - `MemEventStore` — real event store; `insert` assigns monotonic seqs and
//!   writes to the ingest log.
//! - `pull_page_over` — the real kernel scan function used in production.
//! - `PullFeedController` — the 6B controller under test.
//! - `OpFeedEngine` via `register_op_feed` — a real `RootIndexedFeed<...>` that
//!   sorts its snapshot roots by `(created_at, id)` newest-first. Pull events
//!   arrive via `on_kernel_event` just as live relay events do.
//!
//! ## The §1 scenario
//!
//! 1. Insert two "recent" events (high `created_at`). Drain 1 ingests them.
//!    The display snapshot shows [recent_b, recent_a] (newest first).
//! 2. Insert one "late-old" event (very low `created_at = 10`) AFTER the drain.
//!    Its seq (3) is higher than the cursor (2). A `created_at` cursor at 1_000
//!    would skip it; the seq cursor sees it.
//! 3. Drain 2 ingests late_old. The snapshot now contains [recent_b, recent_a,
//!    late_old] — the late event sorts to the bottom because its `created_at` is
//!    smallest, but it IS present: the seq cursor did not skip it.

use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::sync::Arc;

use nmp_planner::InterestShape;
use nmp_store::{EventStore, MemEventStore, PullPage, RawEvent, ScanLogResult, VerifiedEvent};
use nmp_core::{pull_page_over, PullLimits, PullScope};
use nmp_core::KernelEventObserver;
use nmp_feed::{ClosureInterestShape, FeedController, PullFeedController};
use nmp_nip01::op_feed::{build_actor_claim_sink, register_op_feed};

// ─── Fixture helpers ──────────────────────────────────────────────────────────

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const RELAY: &str = "wss://test.relay/";

fn raw_event(id: &str, author: &str, created_at: u64) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind: 1,
        tags: vec![],
        content: format!("note {id}"),
        sig: "00".repeat(64),
    }
}

fn insert(store: &MemEventStore, raw: RawEvent) {
    store
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &RELAY.to_string(),
            1_000,
        )
        .expect("insert must succeed");
}

/// Build a follow-set shape for ALICE only — kind:1/6.
fn alice_shape() -> InterestShape {
    InterestShape {
        authors: [ALICE.to_string()].into_iter().collect::<BTreeSet<_>>(),
        kinds: [1u32, 6u32].into_iter().collect(),
        ..Default::default()
    }
}

/// A no-op claim sink (no actor in this test).
fn noop_sink() -> nmp_feed::ClaimSink {
    build_actor_claim_sink(Arc::new(|_cmd| {}))
}

/// A no-op follow predicate (ALICE self-authored, so follow predicate is
/// irrelevant for root ingestion; a "follows everyone" predicate is fine).
fn follow_all() -> nmp_feed::FollowPredicate {
    Arc::new(|_pubkey: &str| true)
}

/// No-op event lookup (no cross-event repost hydration needed in this test).
fn noop_lookup() -> nmp_feed::EventLookup {
    Arc::new(|_id: &nmp_core::substrate::EventId| None)
}

// ─── Main test ────────────────────────────────────────────────────────────────

#[test]
fn pull_feed_controller_seq_cursor_picks_up_late_old_event() {
    // ── 1. Wire: real store + OP engine + PullFeedController ─────────────────
    let store = Arc::new(MemEventStore::new());

    // OP engine (RootIndexedFeed) — the real production engine.
    let engine = register_op_feed(
        ALICE.to_string(),
        follow_all(),
        noop_lookup(),
        noop_sink(),
    );

    let pull_limits = PullLimits {
        max_entries: NonZeroUsize::new(50).unwrap(),
        max_scan_entries: NonZeroUsize::new(400).unwrap(),
    };

    // Provider: live closure that returns ALICE's shape unconditionally.
    let shape = alice_shape();
    let provider = Arc::new(ClosureInterestShape::new(move || Some(shape.clone())));

    // Pull: real pull_page_over over the MemEventStore.
    let store_for_pull = Arc::clone(&store);
    let pull = Arc::new(move |scope: PullScope, after_seq: u64| {
        match pull_page_over(store_for_pull.as_ref(), scope, after_seq, pull_limits) {
            Ok(result) => result,
            Err(_) => ScanLogResult::Page(PullPage {
                entries: vec![],
                next_after_seq: after_seq,
                latest_seq: after_seq,
                has_more: false,
            }),
        }
    });

    // Apply: deliver drained events through the engine's ingest path.
    let engine_for_apply = engine.clone();
    let apply = Arc::new(move |ev: &nmp_core::substrate::KernelEvent| {
        KernelEventObserver::on_kernel_event(&*engine_for_apply, ev);
    });

    // Advance: grow the engine's visible window after a page is ingested.
    let engine_for_advance = engine.clone();
    let advance = Arc::new(move || {
        engine_for_advance.grow_visible_window();
    });

    // PullFeedController::new always succeeds; fail-closed via provider on load_older.
    let pull_ctrl = PullFeedController::new(provider, pull, apply, advance);

    // ── 2. Insert two "recent" events ────────────────────────────────────────
    // seq 1: recent_a, created_at=1_000
    // seq 2: recent_b, created_at=1_100
    insert(&store, raw_event("a".repeat(64).as_str(), ALICE, 1_000));
    insert(&store, raw_event("b".repeat(64).as_str(), ALICE, 1_100));

    // ── 3. Drain 1: pick up the two recent events ─────────────────────────────
    let changed = pull_ctrl.load_older();
    assert!(changed, "drain 1 must return true (events ingested)");

    // Snapshot after drain 1: [recent_b(ts=1100), recent_a(ts=1000)].
    let snap1 = engine.snapshot_current_window();
    let ids1: Vec<String> = snap1.cards.iter().map(|c| c.card.id.clone()).collect();
    assert_eq!(ids1.len(), 2, "two events in snapshot after drain 1");
    assert_eq!(ids1[0], "b".repeat(64), "newest first: recent_b");
    assert_eq!(ids1[1], "a".repeat(64), "then recent_a");

    // ── 4. Insert the "late-old" event AFTER drain 1 ─────────────────────────
    // created_at = 10 (very old timestamp) but seq = 3 (after the cursor).
    // A `created_at` cursor at 1_000 would silently skip this event (§1 bug).
    // The seq cursor (currently at 2) sees it on the next drain.
    insert(&store, raw_event("c".repeat(64).as_str(), ALICE, 10));

    // ── 5. Drain 2: seq cursor at 2; picks up seq 3 (late_old) ──────────────
    let changed2 = pull_ctrl.load_older();
    assert!(changed2, "drain 2 must return true (late_old ingested)");

    // ── 6. Assert: late_old is now in the snapshot at the RIGHT position ──────
    let snap2 = engine.snapshot_current_window();
    let ids2: Vec<String> = snap2.cards.iter().map(|c| c.card.id.clone()).collect();
    assert_eq!(
        ids2.len(),
        3,
        "all three events must be present after drain 2"
    );
    // Display sort: newest-first by (created_at, id).
    assert_eq!(ids2[0], "b".repeat(64), "position 0: recent_b (ts=1100)");
    assert_eq!(ids2[1], "a".repeat(64), "position 1: recent_a (ts=1000)");
    assert_eq!(
        ids2[2],
        "c".repeat(64),
        "position 2: late_old (ts=10) — seq cursor did NOT skip it (§1 fix)"
    );

    // ── 7. Verify the §1 bug contrast ────────────────────────────────────────
    // A `created_at` cursor pinned at 1_000 (the minimum ts of the displayed events)
    // would skip late_old (ts=10 < 1_000). The seq cursor at 2 sees seq 3
    // regardless of `created_at`. That is the ADR-0058 §1 fix.
    assert!(
        10u64 < 1_000u64,
        "sanity: late_old's created_at IS below the prior display window"
    );
    assert!(
        ids2.contains(&"c".repeat(64)),
        "late_old MUST be in the snapshot — seq cursor covers it"
    );
}
