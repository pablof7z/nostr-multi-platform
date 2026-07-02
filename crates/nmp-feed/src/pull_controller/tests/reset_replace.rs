use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use nmp_core::PullScope;
use nmp_store::ScanLogResult;

use super::{
    controller_with_pages, hex32, inserted, page, real_shape, replaced, FakeFeed,
    PullFeedController, PullFn,
};
use crate::{FeedController, FeedRegistry};

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
        Arc::new(move |_scope: PullScope, after_seq: u64, _limits| {
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
        Arc::new(|_scope, after, _limits| {
            page(vec![inserted(after + 1, "x", 1)], after + 1, after + 1)
        }),
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
        Arc::new(move |_scope: PullScope, after_seq: u64, _limits| {
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
        Arc::new(|_scope, after, _limits| page(vec![], after, after)),
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
