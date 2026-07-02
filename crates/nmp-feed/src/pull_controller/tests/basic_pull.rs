use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_store::{PullGap, ScanLogResult};

use super::{
    controller_with_pages, inserted, opaque_shape, page, real_shape, FakeFeed, PullFeedController,
    PullFn,
};
use crate::{FeedController, FeedLoadStopReason, FeedWindowPolicy};

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

#[test]
fn window_policy_source_page_size_bounds_one_load_older_drain() {
    let feed = Arc::new(FakeFeed::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(vec![
        page(vec![inserted(1, "one", 10)], 1, 4),
        page(vec![inserted(2, "two", 20)], 2, 4),
        page(vec![inserted(3, "three", 30)], 3, 4),
    ])));
    let pull: PullFn = {
        let queue = Arc::clone(&queue);
        let seen = Arc::clone(&seen);
        Arc::new(move |_scope, after_seq, limits| {
            assert_eq!(
                limits.max_entries.get(),
                2usize.saturating_sub(seen.lock().unwrap().len()).max(1),
                "source_page_size must be enforced at the pull boundary"
            );
            seen.lock().unwrap().push(after_seq);
            queue.lock().unwrap().pop_front().unwrap()
        })
    };
    let ctrl = PullFeedController::new_with_perspective_and_window_policy(
        real_shape(),
        pull,
        feed.apply(),
        None,
        None,
        feed.advance(),
        FeedWindowPolicy {
            source_page_size: 2,
            ..FeedWindowPolicy::default()
        },
    );

    let status = ctrl.load_older_status();
    assert!(status.changed);
    assert_eq!(status.reason, FeedLoadStopReason::WindowFilled);
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &[0, 1],
        "source_page_size=2 stops after two accepted rows"
    );
    assert_eq!(
        feed.ingested
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
}

#[test]
fn window_policy_source_scan_budget_bounds_unmatched_work() {
    let feed = Arc::new(FakeFeed::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(vec![
        page(vec![], 1, 3),
        page(vec![inserted(2, "read-on-second-drain", 20)], 2, 3),
    ])));
    let pull: PullFn = {
        let queue = Arc::clone(&queue);
        let seen = Arc::clone(&seen);
        Arc::new(move |_scope, after_seq, limits| {
            assert_eq!(
                limits.max_scan_entries.get(),
                1,
                "source_scan_budget must be enforced at the pull boundary"
            );
            seen.lock().unwrap().push(after_seq);
            queue.lock().unwrap().pop_front().unwrap()
        })
    };
    let ctrl = PullFeedController::new_with_perspective_and_window_policy(
        real_shape(),
        pull,
        feed.apply(),
        None,
        None,
        feed.advance(),
        FeedWindowPolicy {
            source_scan_budget: 1,
            ..FeedWindowPolicy::default()
        },
    );

    let status = ctrl.load_older_status();
    assert!(!status.changed);
    assert_eq!(status.reason, FeedLoadStopReason::SourceScanBudgetReached);
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &[0],
        "source_scan_budget=1 yields after one visited row"
    );
    assert!(feed.ingested.lock().unwrap().is_empty());

    let status = ctrl.load_older_status();
    assert!(status.changed);
    assert_eq!(status.reason, FeedLoadStopReason::SourceScanBudgetReached);
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &[0, 1],
        "the next drain resumes from the budget-advanced cursor"
    );
    assert_eq!(
        feed.ingested
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        ["read-on-second-drain"],
        "repeated load_older reads the next source row without re-scanning"
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
    let status = ctrl.load_older_status();
    assert!(!status.changed, "no new events ⇒ no visible change");
    assert_eq!(status.reason, FeedLoadStopReason::SourceExhausted);
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
    let pull: PullFn = Arc::new(|_scope, _after, _limits| {
        ScanLogResult::Gap(PullGap {
            requested_after_seq: 0,
            first_available_seq: 42,
        })
    });
    let ctrl = PullFeedController::new(real_shape(), pull, feed.apply(), feed.advance());
    let status = ctrl.load_older_status();
    assert!(!status.changed, "a pure gap drained no events");
    assert_eq!(status.reason, FeedLoadStopReason::SourceGap);
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
    let pull: PullFn = Arc::new(|_scope, after, _limits| page(vec![], after, after));
    let ctrl = PullFeedController::new(opaque_shape(), pull, feed.apply(), feed.advance());
    assert!(
        !ctrl.load_older_status().changed,
        "no real shape ⇒ load_older returns false ⇒ projection-only fallback"
    );
    assert_eq!(
        ctrl.load_older_status().reason,
        FeedLoadStopReason::SourceUnavailable
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
        Arc::new(move |_scope, after, _limits| {
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
