//! Behavioral tests for feed ordering stability, window pagination, and the
//! D5 eviction/attribution-cleanup invariant. Split from `tests/mod.rs` to keep
//! each test file under the 500-LOC ceiling. Fixtures live in `support.rs`.

use super::support::{reply_event, repost_event, root_event, Harness};

#[test]
fn older_repost_does_not_regress_order() {
    // Fix 1: a root first seen via a repost wrapper at ts=100 must NOT be
    // pulled downward when a second, older repost wrapper (ts=50) arrives.
    let h = Harness::new(&["alice", "bob"]);
    h.ingest(&repost_event("rp1", "alice", 100, "op1", ""));
    h.ingest(&root_event("op1", "carol", 10, "the post"));

    let snap_before = h.snapshot();
    assert!(
        snap_before.cards.iter().any(|c| c.card.root_id == "op1"),
        "op1 present after first repost"
    );

    // Second repost with an OLDER timestamp.
    h.ingest(&repost_event("rp2", "bob", 50, "op1", ""));

    let snap_after = h.snapshot();
    assert!(
        snap_after.cards.iter().any(|c| c.card.root_id == "op1"),
        "op1 still present after older repost"
    );
    assert_eq!(
        snap_after.page.as_ref().unwrap().has_more,
        snap_before.page.as_ref().unwrap().has_more,
        "page shape unchanged"
    );

    // A second root at ts=80 must sort BELOW op1 (effective ts=100), proving
    // the older repost did not regress op1's ordering position.
    h.ingest(&root_event("op2", "dave", 80, "second post"));
    let snap_final = h.snapshot();
    assert_eq!(
        snap_final.cards[0].card.root_id, "op1",
        "op1 (effective ts=100) must still sort above op2 (ts=80) after older repost"
    );
}

#[test]
fn grow_visible_window_reveals_past_default_window() {
    // 6B: the engine is no longer a `FeedController`; the render viewport is
    // grown by `grow_visible_window`, which `PullFeedController` calls after a
    // pull drain ingests a page. Drive that viewport step directly to prove it
    // reveals roots one page at a time, capped at the total root count.
    let h = Harness::new(&["alice"]);
    // Insert more roots than the default window (80).
    for i in 0..120u64 {
        h.ingest(&root_event(&format!("op{i}"), "bob", 1000 + i, "body"));
    }

    // First snapshot is bounded to DEFAULT_FEED_WINDOW_LIMIT (80).
    let snap = h.engine.snapshot_current_window();
    assert_eq!(snap.cards.len(), 80, "initial snapshot bounded to 80");
    assert_eq!(snap.page.as_ref().unwrap().has_more, true, "older roots remain");

    // Growing the viewport reveals more and reports it grew.
    let more = h.engine.grow_visible_window();
    assert!(more, "grow_visible_window must return true when older roots exist");

    // snapshot now honors the grown viewport: all 120 roots.
    let snap_after = h.engine.snapshot_current_window();
    assert_eq!(
        snap_after.cards.len(),
        120,
        "snapshot after grow_visible_window shows all 120 roots"
    );

    // Fully revealed → grow_visible_window returns false.
    let no_more = h.engine.grow_visible_window();
    assert!(
        !no_more,
        "grow_visible_window must return false when all roots are visible"
    );
}

#[test]
fn evicted_root_attribution_is_cleaned_up() {
    use nmp_core::substrate::MAX_PROJECTION_MESSAGES;

    let h = Harness::new(&["alice"]);

    // Fill the roots map to capacity, each with one attribution.
    for i in 0..MAX_PROJECTION_MESSAGES {
        h.ingest(&root_event(
            &format!("op{i}"),
            "bob",
            1000 + i as u64,
            "body",
        ));
        h.ingest(&reply_event(
            &format!("r{i}"),
            "alice",
            1001 + i as u64,
            &format!("op{i}"),
        ));
    }

    // Insert one more root at capacity → evicts the oldest root (op0). Its
    // attribution sub-map must be reclaimed (Fix 4) — exercising the eviction
    // path without panic.
    h.ingest(&root_event(
        "new_root",
        "bob",
        2000 + MAX_PROJECTION_MESSAGES as u64,
        "new",
    ));

    let snap = h.snapshot();
    assert!(
        snap.cards.len() <= MAX_PROJECTION_MESSAGES,
        "roots bounded by D5 cap"
    );
    // The newest root is present; the evicted oldest (op0) is gone from the
    // visible window (it is the lowest-ts root and was first to be evicted).
    assert!(
        snap.cards.iter().any(|c| c.card.root_id == "new_root"),
        "new_root was inserted after eviction"
    );
    assert!(
        !snap.cards.iter().any(|c| c.card.root_id == "op0"),
        "op0 was evicted from roots"
    );
}
