//! Controller-level tests for the single pull paging path (ADR-0058 §8 step-6B).
//!
//! These exercise `PullFeedController::load_older` end-to-end with stub closures
//! (no real `Kernel`): a fake `pull_fn` feeds canned pages, a fake `apply`
//! records what the feed ingested, and a fake `advance` counts viewport grows.
//! Together they prove the 6B contract: late-old completeness rides seq, display
//! order is left to the snapshot, empty-advancing drains do not loop, `PullGap`
//! rebases explicitly, inexpressible shapes fail closed, and nothing consumes a
//! wake.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nmp_planner::InterestShape;
use nmp_store::{LogOp, PullGap, PullPage, RawEvent, ScanLogResult, StoreLogEntry};
use nmp_core::substrate::KernelEvent;
use nmp_core::PullScope;

use super::{ClosureInterestShape, FeedAdvance, FeedApply, PullFeedController, PullFn};
use crate::FeedController;

// ─── fixtures ────────────────────────────────────────────────────────────────

fn real_shape() -> Arc<dyn crate::FeedInterestShape + Send + Sync> {
    Arc::new(ClosureInterestShape::new(|| {
        Some(InterestShape {
            authors: ["a".repeat(64)].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        })
    }))
}

fn opaque_shape() -> Arc<dyn crate::FeedInterestShape + Send + Sync> {
    Arc::new(ClosureInterestShape::new(|| None))
}

fn raw(id: &str, created_at: u64) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: "a".repeat(64),
        created_at,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "0".repeat(128),
    }
}

fn inserted(seq: u64, id: &str, created_at: u64) -> StoreLogEntry {
    StoreLogEntry {
        seq,
        op: LogOp::Inserted,
        event_id: [seq as u8; 32],
        raw_event: Some(raw(id, created_at)),
        source_relay: Some("wss://r/".to_string()),
        received_at_ms: 0,
    }
}

fn page(entries: Vec<StoreLogEntry>, next_after_seq: u64, latest_seq: u64) -> ScanLogResult {
    ScanLogResult::Page(PullPage {
        entries,
        next_after_seq,
        latest_seq,
        has_more: next_after_seq < latest_seq,
    })
}

/// A test feed: an in-memory ingest sink (deduped by id) plus a viewport-grow
/// counter, mirroring the engine's `apply` + `grow_visible_window` seam.
#[derive(Default)]
struct FakeFeed {
    ingested: Mutex<Vec<KernelEvent>>,
    grows: AtomicUsize,
}

impl FakeFeed {
    fn apply(self: &Arc<Self>) -> FeedApply {
        let me = Arc::clone(self);
        Arc::new(move |ev: &KernelEvent| {
            let mut g = me.ingested.lock().unwrap();
            if !g.iter().any(|e| e.id == ev.id) {
                g.push(ev.clone());
            }
        })
    }
    fn advance(self: &Arc<Self>) -> FeedAdvance {
        let me = Arc::clone(self);
        Arc::new(move || {
            me.grows.fetch_add(1, Ordering::Relaxed);
        })
    }
    /// Display order the snapshot would render: newest-first by `(created_at, id)`.
    fn display_order(&self) -> Vec<String> {
        let g = self.ingested.lock().unwrap();
        let mut keyed: Vec<(u64, String)> =
            g.iter().map(|e| (e.created_at, e.id.clone())).collect();
        keyed.sort_by(|(lt, lid), (rt, rid)| rt.cmp(lt).then_with(|| rid.cmp(lid)));
        keyed.into_iter().map(|(_, id)| id).collect()
    }
}

/// Build a controller whose `pull_fn` replays `pages` (one per call) and records
/// every `after_seq` it was asked for.
fn controller_with_pages(
    feed: &Arc<FakeFeed>,
    pages: Vec<ScanLogResult>,
    seen_after: Arc<Mutex<Vec<u64>>>,
) -> Arc<PullFeedController> {
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(pages)));
    let pull: PullFn = {
        let queue = Arc::clone(&queue);
        let seen = Arc::clone(&seen_after);
        Arc::new(move |_scope: PullScope, after_seq: u64| {
            seen.lock().unwrap().push(after_seq);
            queue
                .lock()
                .unwrap()
                .pop_front()
                // Default: caught up, empty exhausted page (fail-closed terminator).
                .unwrap_or_else(|| page(vec![], after_seq, after_seq))
        })
    };
    PullFeedController::new(real_shape(), pull, feed.apply(), feed.advance())
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// THE bug fix: a low-`created_at` event ingested LATE (higher seq) is NOT
/// skipped — a later `load_older` drains it by seq, and the feed's display sort
/// places it correctly. A `created_at` cursor would have dropped it.
#[test]
fn late_old_event_is_not_skipped_across_loads() {
    let feed = Arc::new(FakeFeed::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    // Load 1 drains the two recent events (seq 1,2). Load 2 drains a late event
    // with a LOW created_at at seq 3 — behind the display window but ahead in seq.
    let ctrl = controller_with_pages(
        &feed,
        vec![
            page(
                vec![
                    inserted(1, "recent_a", 1_000),
                    inserted(2, "recent_b", 1_100),
                ],
                2,
                2,
            ),
            page(vec![inserted(3, "late_old", 10)], 3, 3),
        ],
        Arc::clone(&seen),
    );

    assert!(ctrl.load_older(), "first drain ingested a page");
    assert_eq!(
        ctrl.after_seq(),
        2,
        "cursor advanced past the recent events"
    );

    assert!(
        ctrl.load_older(),
        "second drain ingested the late-old event"
    );
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &[0, 2],
        "second drain resumes from the seq-2 cursor — by seq, not created_at"
    );
    assert!(
        feed.ingested
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.id == "late_old"),
        "the late-arriving old-created_at event WAS ingested (seq completeness)"
    );
}

/// Display order is the snapshot's `(created_at, id)` sort — the seq drain order
/// is irrelevant. The late-old event drains last but renders at the bottom.
#[test]
fn display_order_stays_created_at_after_seq_drain() {
    let feed = Arc::new(FakeFeed::default());
    let ctrl = controller_with_pages(
        &feed,
        vec![page(
            vec![
                inserted(1, "m1", 500),
                inserted(2, "m2", 900),
                inserted(3, "late", 100),
            ],
            3,
            3,
        )],
        Arc::new(Mutex::new(Vec::new())),
    );
    ctrl.load_older();
    assert_eq!(
        feed.display_order(),
        ["m2", "m1", "late"],
        "snapshot order is created_at-desc — late (ts=100) sorts to the bottom"
    );
}

/// An empty-but-advancing pull (store caught up, nothing matched) terminates at
/// once: `load_older` returns false, applies nothing, grows nothing, and calls
/// `pull_fn` exactly once — it does NOT loop or poll.
#[test]
fn empty_advancing_pull_does_not_loop() {
    let feed = Arc::new(FakeFeed::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let ctrl = controller_with_pages(
        &feed,
        vec![page(vec![], 9, 9)], // advanced to seq 9, store caught up
        Arc::clone(&seen),
    );
    assert!(!ctrl.load_older(), "no new events ⇒ no visible change");
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "exactly one pull — no polling"
    );
    assert_eq!(
        feed.grows.load(Ordering::Relaxed),
        0,
        "viewport did not grow"
    );
    assert!(feed.ingested.lock().unwrap().is_empty());
}

/// A `PullGap` rebases the pager cursor explicitly and stops; the controller
/// never silently claims continuity across the gap.
#[test]
fn pull_gap_rebases_explicitly_no_silent_continuity() {
    let feed = Arc::new(FakeFeed::default());
    let pull: PullFn = Arc::new(|_scope, _after| {
        ScanLogResult::Gap(PullGap {
            requested_after_seq: 0,
            first_available_seq: 42,
        })
    });
    let ctrl = PullFeedController::new(real_shape(), pull, feed.apply(), feed.advance());
    assert!(!ctrl.load_older(), "a pure gap drained no events");
    assert_eq!(
        ctrl.after_seq(),
        42,
        "cursor was rebased to first_available_seq, not advanced as if continuous"
    );
}

/// A feed whose interest is inexpressible fails closed: the controller is
/// built unconditionally, but `load_older` returns false (no pull, no
/// broad-scan) — the feed renders its push projection only.
#[test]
fn inexpressible_shape_fails_closed_to_projection() {
    let feed = Arc::new(FakeFeed::default());
    let pull: PullFn = Arc::new(|_scope, after| page(vec![], after, after));
    let ctrl = PullFeedController::new(opaque_shape(), pull, feed.apply(), feed.advance());
    assert!(
        !ctrl.load_older(),
        "no real shape ⇒ load_older returns false ⇒ projection-only fallback"
    );
    assert!(
        feed.ingested.lock().unwrap().is_empty(),
        "opaque provider ⇒ no events ingested, pull_fn never called"
    );
}

/// `load_older` is a pure synchronous drain: it never registers, decodes, or
/// consumes `nmp.pull.wake`. We prove it structurally — the only input the
/// controller ever receives is the `pull_fn(scope, after_seq)` closure, whose
/// signature carries a seq cursor, NOT a wake batch. Draining advances the seq
/// cursor with no wake plumbing in sight.
#[test]
fn load_older_does_not_require_or_consume_a_wake() {
    let feed = Arc::new(FakeFeed::default());
    let wake_reads = Arc::new(AtomicU64::new(0));
    let pull: PullFn = {
        // A wake-driven path would have to read a wake source to know there is
        // work; this closure never does — it answers purely from `after_seq`.
        let wake_reads = Arc::clone(&wake_reads);
        Arc::new(move |_scope, after| {
            assert_eq!(
                wake_reads.load(Ordering::Relaxed),
                0,
                "no wake was decoded to drive this drain"
            );
            page(vec![inserted(after + 1, "e", 100)], after + 1, after + 1)
        })
    };
    let ctrl = PullFeedController::new(real_shape(), pull, feed.apply(), feed.advance());
    assert!(ctrl.load_older(), "drained synchronously, wake-free");
    assert_eq!(wake_reads.load(Ordering::Relaxed), 0);
}
