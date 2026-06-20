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

use nmp_core::substrate::KernelEvent;
use nmp_core::PullScope;
use nmp_planner::InterestShape;
use nmp_store::{LogOp, PullGap, PullPage, RawEvent, ScanLogResult, StoreLogEntry};

use super::{
    ClosureInterestShape, FeedAdvance, FeedApply, FeedReplace, FeedReset, PullFeedController,
    PullFn,
};
use crate::{FeedController, FeedRegistry};

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

/// A `Replaced` log row: a replaceable event (`id`/`created_at`) superseding an
/// earlier version whose 32-byte id is all-`replaced_byte`. The superseded id's
/// hex form is what the controller hands the `FeedReplace` hook.
fn replaced(seq: u64, id: &str, created_at: u64, replaced_byte: u8) -> StoreLogEntry {
    StoreLogEntry {
        seq,
        op: LogOp::Replaced {
            replaced_id: [replaced_byte; 32],
        },
        event_id: [seq as u8; 32],
        raw_event: Some(raw(id, created_at)),
        source_relay: Some("wss://r/".to_string()),
        received_at_ms: 0,
    }
}

/// The lowercase-hex id the controller derives from an all-`byte` 32-byte id.
fn hex32(byte: u8) -> String {
    (0..32).map(|_| format!("{byte:02x}")).collect()
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
    resets: AtomicUsize,
    removed: Mutex<Vec<String>>,
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
    /// A reset hook that drops all ingested rows, counts each call, and reports
    /// whether anything was actually cleared — mirroring
    /// `FlatFeed::reset_for_perspective_change`.
    fn reset(self: &Arc<Self>) -> FeedReset {
        let me = Arc::clone(self);
        Arc::new(move || {
            me.resets.fetch_add(1, Ordering::Relaxed);
            let mut g = me.ingested.lock().unwrap();
            let had_rows = !g.is_empty();
            g.clear();
            had_rows
        })
    }
    /// A replace hook that evicts a source by id and records the eviction.
    fn replace(self: &Arc<Self>) -> FeedReplace {
        let me = Arc::clone(self);
        Arc::new(move |source_id: &str| {
            me.removed.lock().unwrap().push(source_id.to_string());
            me.ingested.lock().unwrap().retain(|e| e.id != source_id);
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

// ─── perspective reset / replacement (Olas PR #39) ─────────────────────────────

/// Build a controller over `pages` with the reset + replace perspective hooks
/// wired (`new_with_perspective`), recording every `after_seq` it pulls.
fn controller_with_perspective(
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
                .unwrap_or_else(|| page(vec![], after_seq, after_seq))
        })
    };
    PullFeedController::new_with_perspective(
        real_shape(),
        pull,
        feed.apply(),
        Some(feed.replace()),
        Some(feed.reset()),
        feed.advance(),
    )
}

/// `reset` rewinds the cursor to seq 0, calls the feed reset hook exactly once,
/// returns whether visible state changed, and the next `load_older` replays
/// from seq 0 — the whole perspective-change contract in one test.
#[test]
fn reset_rewinds_cursor_clears_state_once_and_replays_from_zero() {
    let feed = Arc::new(FakeFeed::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let ctrl = controller_with_perspective(
        &feed,
        vec![
            // Load 1: drain two rows, cursor → 2.
            page(vec![inserted(1, "a", 100), inserted(2, "b", 200)], 2, 2),
            // Load 2 (after reset): a fresh page served from seq 0 again.
            page(vec![inserted(1, "a", 100)], 1, 1),
        ],
        Arc::clone(&seen),
    );

    assert!(ctrl.load_older(), "first drain ingested a page");
    assert_eq!(ctrl.after_seq(), 2, "cursor advanced past the drained rows");

    // The reset hook reports that visible state changed (the window had rows),
    // and it is invoked exactly once.
    assert!(
        ctrl.reset(),
        "reset cleared a non-empty window ⇒ visible change"
    );
    assert_eq!(
        feed.resets.load(Ordering::Relaxed),
        1,
        "feed reset called once"
    );
    assert_eq!(
        ctrl.after_seq(),
        0,
        "reset rewound the pull cursor to seq 0"
    );
    assert!(
        feed.ingested.lock().unwrap().is_empty(),
        "reset dropped the stale window"
    );

    // The next load replays from seq 0 — the rewound cursor, not the old seq-2.
    assert!(ctrl.load_older(), "post-reset drain refilled the window");
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &[0, 0],
        "both drains started at seq 0 — reset replayed history, did not resume"
    );
}

/// `reset` over an already-empty window still rewinds the cursor but reports no
/// visible change (the reset hook returns false).
#[test]
fn reset_reports_no_change_on_empty_window() {
    let feed = Arc::new(FakeFeed::default());
    let ctrl = controller_with_perspective(&feed, vec![], Arc::new(Mutex::new(Vec::new())));
    assert!(
        !ctrl.reset(),
        "nothing visible to clear ⇒ reset reports no change"
    );
    assert_eq!(
        feed.resets.load(Ordering::Relaxed),
        1,
        "hook still invoked once"
    );
    assert_eq!(ctrl.after_seq(), 0, "cursor remained/rewound at seq 0");
}

/// A controller built without a reset hook (`new_with_replacement`) still
/// rewinds the cursor on `reset`, but reports no visible change and never
/// fabricates a feed-reset call.
#[test]
fn reset_without_hook_only_rewinds_cursor() {
    let feed = Arc::new(FakeFeed::default());
    let ctrl = PullFeedController::new_with_replacement(
        real_shape(),
        Arc::new(|_scope, after| page(vec![inserted(after + 1, "x", 1)], after + 1, after + 1)),
        feed.apply(),
        feed.replace(),
        feed.advance(),
    );
    assert!(ctrl.load_older(), "drained one row");
    assert_eq!(ctrl.after_seq(), 1, "cursor advanced");
    assert!(
        !ctrl.reset(),
        "no reset hook ⇒ reset reports no visible change"
    );
    assert_eq!(ctrl.after_seq(), 0, "but the cursor was still rewound");
    assert_eq!(
        feed.resets.load(Ordering::Relaxed),
        0,
        "no feed reset fabricated"
    );
}

/// A poisoned pager fails the reset closed: no rewind attempt succeeds, the
/// feed reset hook is NOT called, and `reset` returns false — never a panic.
#[test]
fn reset_fails_closed_on_poisoned_pager() {
    let feed = Arc::new(FakeFeed::default());
    let ctrl = controller_with_perspective(&feed, vec![], Arc::new(Mutex::new(Vec::new())));
    // Poison the pager mutex by panicking while it is held.
    let poisoner = Arc::clone(&ctrl);
    let _ = std::thread::spawn(move || {
        let _guard = poisoner.pager.lock().unwrap();
        panic!("poison the pager");
    })
    .join();

    assert!(!ctrl.reset(), "poisoned pager ⇒ reset fails closed (false)");
    assert_eq!(
        feed.resets.load(Ordering::Relaxed),
        0,
        "fail closed: the feed reset hook is never reached"
    );
}

/// `new_with_replacement` evicts a superseded source during a drain: a
/// `LogOp::Replaced` row hands the prior version's hex id to the replace hook,
/// while the superseding version ingests normally.
#[test]
fn new_with_replacement_evicts_superseded_source() {
    let feed = Arc::new(FakeFeed::default());
    // seq 1 inserts the original, whose event id is the hex of all-0xAA bytes;
    // seq 2 is its replacement, whose `Replaced` row supersedes that 0xAA id.
    // The feed keys sources by the (hex) event id, so eviction must match it.
    let old_id = hex32(0xAA);
    let pages = vec![
        page(vec![inserted(1, &old_id, 100)], 1, 1),
        page(vec![replaced(2, "new", 200, 0xAA)], 2, 2),
    ];
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(pages)));
    let pull: PullFn = {
        let queue = Arc::clone(&queue);
        Arc::new(move |_scope: PullScope, after_seq: u64| {
            queue
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| page(vec![], after_seq, after_seq))
        })
    };
    let ctrl = PullFeedController::new_with_replacement(
        real_shape(),
        pull,
        feed.apply(),
        feed.replace(),
        feed.advance(),
    );

    assert!(ctrl.load_older(), "first drain ingested the original");
    assert!(ctrl.load_older(), "second drain ingested the replacement");

    assert_eq!(
        feed.removed.lock().unwrap().as_slice(),
        &[hex32(0xAA)],
        "the superseded id (hex) was handed to the replace hook"
    );
    let ids: Vec<String> = feed
        .ingested
        .lock()
        .unwrap()
        .iter()
        .map(|e| e.id.clone())
        .collect();
    assert_eq!(
        ids,
        ["new"],
        "the stale source was evicted; only the replacement renders"
    );
}

/// Key-driven eviction: `FeedRegistry::replace(key, id)` must reach a real
/// `PullFeedController`'s wired replace hook — NOT the `FeedController::replace_source`
/// trait default. A controller built with `new_with_replacement` accepts and
/// forwards the id verbatim. (Olas drives eviction this way: by feed key, not
/// only from a drained `LogOp::Replaced` row.)
#[test]
fn registry_replace_reaches_new_with_replacement_hook() {
    let feed = Arc::new(FakeFeed::default());
    let ctrl = PullFeedController::new_with_replacement(
        real_shape(),
        Arc::new(|_scope, after| page(vec![], after, after)),
        feed.apply(),
        feed.replace(),
        feed.advance(),
    );
    let reg = FeedRegistry::default();
    reg.register("nmp.feed.olas.photos", ctrl);

    assert!(
        reg.replace("nmp.feed.olas.photos", "deadbeef"),
        "registry.replace reached the wired hook ⇒ accepted"
    );
    assert_eq!(
        feed.removed.lock().unwrap().as_slice(),
        &["deadbeef".to_string()],
        "the source id was forwarded to the replace hook verbatim"
    );
}

/// The same key-driven eviction works for a controller built with
/// `new_with_perspective` (both hooks wired).
#[test]
fn registry_replace_reaches_new_with_perspective_hook() {
    let feed = Arc::new(FakeFeed::default());
    let ctrl = controller_with_perspective(&feed, vec![], Arc::new(Mutex::new(Vec::new())));
    let reg = FeedRegistry::default();
    reg.register("nmp.feed.olas.photos", ctrl);

    assert!(
        reg.replace("nmp.feed.olas.photos", "cafe"),
        "registry.replace reached the perspective controller's replace hook"
    );
    assert_eq!(
        feed.removed.lock().unwrap().as_slice(),
        &["cafe".to_string()],
        "the source id was forwarded verbatim"
    );
}

/// A controller built with plain `new` wires no replace hook, so key-driven
/// eviction fails closed (`false`) rather than silently claiming acceptance —
/// the regression this task fixes (the trait default returned false WITHOUT
/// ever consulting a wired hook; a real controller must consult its own).
#[test]
fn registry_replace_without_hook_fails_closed() {
    let feed = Arc::new(FakeFeed::default());
    let ctrl = controller_with_pages(&feed, vec![], Arc::new(Mutex::new(Vec::new())));
    let reg = FeedRegistry::default();
    reg.register("nmp.feed.olas.photos", ctrl);

    assert!(
        !reg.replace("nmp.feed.olas.photos", "deadbeef"),
        "no replace hook wired ⇒ key-driven eviction is an honest no-op (false)"
    );
    assert!(
        feed.removed.lock().unwrap().is_empty(),
        "nothing was evicted — no hook to consult"
    );
}
