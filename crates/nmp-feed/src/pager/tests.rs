//! Unit tests for the feed pull pager (ADR-0058 §8 step-6A).
//!
//! All tests use stub `pull_fn`s — no real `Kernel`. They exercise the
//! 6A-testable subset: late-old-event completeness, empty-advancing-page
//! termination, gap rebase, fail-closed shape, and display-order invariance.

use super::*;
use nmp_core::store::{LogOp, PullGap, PullPage, RawEvent, ScanLogResult, StoreLogEntry};

// ─── fixtures ────────────────────────────────────────────────────────────────

/// A feed whose interest is a real shape (kind:1 from one author).
struct RealShapeFeed;
impl FeedInterestShape for RealShapeFeed {
    fn interest_shape(&self) -> Option<InterestShape> {
        Some(InterestShape {
            authors: ["a".repeat(64)].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        })
    }
}

/// A feed that cannot express its interest — fails closed.
struct OpaqueFeed;
impl FeedInterestShape for OpaqueFeed {
    fn interest_shape(&self) -> Option<InterestShape> {
        None
    }
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

/// The store-log `EventId` is a `[u8; 32]` (distinct from the `RawEvent`/
/// `KernelEvent` hex-string id). The adapter reads the raw event, never this
/// field, so the tests use a deterministic-but-opaque byte id.
fn id32(seed: u8) -> [u8; 32] {
    [seed; 32]
}

fn inserted(seq: u64, id: &str, created_at: u64) -> StoreLogEntry {
    StoreLogEntry {
        seq,
        op: LogOp::Inserted,
        event_id: id32(seq as u8),
        raw_event: Some(raw(id, created_at)),
        source_relay: Some("wss://r/".to_string()),
        received_at_ms: 0,
    }
}

fn deleted(seq: u64, _target: &str) -> StoreLogEntry {
    StoreLogEntry {
        seq,
        op: LogOp::Deleted {
            target_id: id32(seq as u8),
            reason: nmp_core::store::DeleteReason::Nip09,
        },
        event_id: id32(0xff),
        raw_event: None,
        source_relay: None,
        received_at_ms: 0,
    }
}

/// A single-shot page covering the whole log (`has_more` set from `latest`).
fn one_page(entries: Vec<StoreLogEntry>, next_after_seq: u64, latest_seq: u64) -> ScanLogResult {
    ScanLogResult::Page(PullPage {
        entries,
        next_after_seq,
        latest_seq,
        has_more: next_after_seq < latest_seq,
    })
}

/// Engine-equivalent display sort: newest-first by `(created_at, id)`
/// (mirrors `root_indexed/engine/mod.rs:327`).
fn display_sorted(events: &[KernelEvent]) -> Vec<String> {
    let mut keyed: Vec<(u64, String)> = events
        .iter()
        .map(|e| (e.created_at, e.id.clone()))
        .collect();
    keyed.sort_by(|(lt, lid), (rt, rid)| rt.cmp(lt).then_with(|| rid.cmp(lid)));
    keyed.into_iter().map(|(_, id)| id).collect()
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// THE key correctness test: an event with a LOW `created_at` ingested LATE
/// (so a HIGHER seq, behind the display cursor) is still drained, because pull
/// completeness rides ingest seq, not `created_at`. A `created_at` cursor would
/// silently skip it (ADR-0058 §1).
#[test]
fn test_late_old_event_not_skipped() {
    let mut pager = FeedPullPager::new(&RealShapeFeed).expect("real shape");
    // seq 1,2 are recent; seq 3 is an OLD event (created_at=10) that arrived
    // late — its created_at is behind the others but its seq is ahead.
    let page = one_page(
        vec![
            inserted(1, "recent_a", 1_000),
            inserted(2, "recent_b", 1_100),
            inserted(3, "late_old", 10),
        ],
        3,
        3,
    );
    let mut once = Some(page);
    let out = pager.drain(|_after| once.take().expect("pulled once"));

    let ids: Vec<&str> = out.events.iter().map(|e| e.id.as_str()).collect();
    // The late-old event is present — NOT skipped despite its old created_at.
    assert!(
        ids.contains(&"late_old"),
        "late old event must be drained by seq, got {ids:?}"
    );
    // Drain order is ingest-seq order: late_old comes LAST (highest seq).
    assert_eq!(ids, ["recent_a", "recent_b", "late_old"]);
    assert_eq!(pager.after_seq(), 3);
    assert_eq!(out.stop, DrainStop::Exhausted);
}

/// An empty-but-advancing page (cursor moved over non-matching rows, store
/// caught up) terminates immediately — it does not loop or poll.
#[test]
fn test_empty_advancing_page_terminates() {
    let mut pager = FeedPullPager::new(&RealShapeFeed).expect("real shape");
    // No matching entries, but the cursor advanced to seq 5 and the store is
    // caught up (next_after_seq == latest_seq ⇒ has_more == false).
    let mut calls = 0u32;
    let out = pager.drain(|_after| {
        calls += 1;
        one_page(vec![], 5, 5)
    });

    assert_eq!(calls, 1, "must not poll/loop on an advancing empty page");
    assert!(out.events.is_empty());
    assert_eq!(pager.after_seq(), 5, "cursor advanced over non-matches");
    assert_eq!(out.stop, DrainStop::Exhausted);
}

/// A `PullGap` triggers an explicit cursor rebase and a `Gap` stop — the pager
/// never silently claims continuity across the gap.
#[test]
fn test_pullgap_triggers_rebase() {
    let mut pager = FeedPullPager::new(&RealShapeFeed)
        .expect("real shape")
        .with_budgets(80, 64);
    // Pretend the cursor was behind the GC floor.
    let out = pager.drain(|_after| {
        ScanLogResult::Gap(PullGap {
            requested_after_seq: 0,
            first_available_seq: 42,
        })
    });

    assert_eq!(
        out.stop,
        DrainStop::Gap { rebased_to: 42 },
        "gap must surface explicitly"
    );
    assert_eq!(pager.after_seq(), 42, "cursor rebased to first_available_seq");
    assert!(out.events.is_empty());
}

/// The pager requires a real `InterestShape`. A feed that fails closed
/// (`interest_shape() == None`) yields no pager — there is no broad-scan path.
#[test]
fn test_unsupported_shape_fails_closed() {
    assert!(
        FeedPullPager::new(&OpaqueFeed).is_none(),
        "opaque feed must fail closed — no pager, no broad-scan"
    );
    assert!(
        FeedPullPager::new(&RealShapeFeed).is_some(),
        "real shape constructs a pager"
    );
}

/// Display order stays `(created_at, id)` newest-first even though the seq drain
/// delivers the late-old event last. Completeness rides seq; display rides
/// created_at — the two are independent.
#[test]
fn test_display_order_after_seq_drain() {
    let mut pager = FeedPullPager::new(&RealShapeFeed).expect("real shape");
    // Drain order (by seq): m1(ts=500), m2(ts=900), late(ts=100).
    let page = one_page(
        vec![
            inserted(1, "m1", 500),
            inserted(2, "m2", 900),
            inserted(3, "late", 100),
        ],
        3,
        3,
    );
    let mut once = Some(page);
    let out = pager.drain(|_after| once.take().expect("pulled once"));

    // Drain (seq) order: late is LAST.
    let drain_ids: Vec<&str> = out.events.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(drain_ids, ["m1", "m2", "late"]);

    // Display (created_at desc, id desc) order: late sorts to the BOTTOM.
    let shown = display_sorted(&out.events);
    assert_eq!(shown, ["m2", "m1", "late"]);
    // The pager did not reorder for display — the two orders differ.
    assert_ne!(drain_ids, shown.iter().map(String::as_str).collect::<Vec<_>>());
}

/// `Deleted` rows never become events (InterestShape pull is positive-only,
/// ADR-0058 §10) but still advance the cursor.
#[test]
fn test_deleted_rows_skipped_but_advance() {
    let mut pager = FeedPullPager::new(&RealShapeFeed).expect("real shape");
    let page = one_page(
        vec![inserted(1, "keep", 100), deleted(2, "keep")],
        2,
        2,
    );
    let mut once = Some(page);
    let out = pager.drain(|_after| once.take().expect("pulled once"));

    let ids: Vec<&str> = out.events.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["keep"], "deleted row must not yield an event");
    assert_eq!(pager.after_seq(), 2, "cursor advanced past the deleted row");
}

/// A multi-page drain stops once the visible target grows by one page; the
/// cursor advances only after each page is processed.
#[test]
fn test_page_filled_stops_drain() {
    let mut pager = FeedPullPager::new(&RealShapeFeed)
        .expect("real shape")
        .with_budgets(2, 64);
    // Two matching rows fill the 2-event page; has_more stays true.
    let out = pager.drain(|after| {
        let base = after;
        one_page(
            vec![
                inserted(base + 1, &format!("e{}", base + 1), 100 + base),
                inserted(base + 2, &format!("e{}", base + 2), 100 + base),
            ],
            base + 2,
            1_000, // latest far ahead ⇒ has_more
        )
    });

    assert_eq!(out.events.len(), 2);
    assert_eq!(out.stop, DrainStop::PageFilled);
    assert_eq!(pager.after_seq(), 2);
}
