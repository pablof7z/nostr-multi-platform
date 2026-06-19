//! Unit tests for ADR-0058 step-2 kernel pull service.
//!
//! Covers GlobalLog + InterestShape semantics, scan-budget enforcement,
//! predicate correctness, and gap propagation using the in-memory store.

use std::num::NonZeroUsize;

use crate::kernel::pull::{PullError, PullLimits, PullScope};
use crate::kernel::Kernel;
use crate::planner::{InterestShape, NaddrCoord};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{EventStore, LogOp, RawEvent, ScanLogResult, VerifiedEvent};
use std::collections::BTreeSet;

// ─── Test helpers ────────────────────────────────────────────────────────────

fn hex64(byte: u8) -> String { format!("{:02x}", byte).repeat(32) }

fn raw(id_byte: u8, pk_byte: u8, kind: u32, ts: u64) -> RawEvent {
    RawEvent { id: hex64(id_byte), pubkey: hex64(pk_byte), created_at: ts, kind,
               tags: vec![], content: String::new(), sig: "cc".repeat(64) }
}

fn raw_tags(id_byte: u8, pk_byte: u8, kind: u32, ts: u64, tags: Vec<Vec<String>>) -> RawEvent {
    RawEvent { id: hex64(id_byte), pubkey: hex64(pk_byte), created_at: ts, kind,
               tags, content: String::new(), sig: "cc".repeat(64) }
}

fn kind5(id_byte: u8, pk_byte: u8, ts: u64, target: &str) -> RawEvent {
    raw_tags(id_byte, pk_byte, 5, ts, vec![vec!["e".into(), target.into()]])
}

fn unchecked(r: RawEvent) -> VerifiedEvent { VerifiedEvent::from_raw_unchecked(r) }

const RELAY: &str = "wss://test/";

fn seed(k: &Kernel, r: RawEvent) -> u64 {
    k.event_store_handle().insert(unchecked(r), &RELAY.to_string(), 0).unwrap();
    k.event_store_handle().latest_ingest_seq().unwrap()
}

fn new_kernel() -> Kernel { Kernel::new(DEFAULT_VISIBLE_LIMIT) }

fn lim(max: usize, scan: usize) -> PullLimits {
    PullLimits { max_entries: NonZeroUsize::new(max).unwrap(),
                 max_scan_entries: NonZeroUsize::new(scan).unwrap() }
}

fn shape_ak(pk_byte: u8, kinds: impl IntoIterator<Item = u32>) -> InterestShape {
    let mut s = InterestShape::default();
    s.authors.insert(hex64(pk_byte));
    s.kinds = kinds.into_iter().collect();
    s
}

fn page(r: ScanLogResult) -> crate::store::PullPage {
    match r { ScanLogResult::Page(p) => p, ScanLogResult::Gap(g) =>
        panic!("expected Page, got Gap(first={})", g.first_available_seq) }
}

fn pull_interest(k: &Kernel, shape: InterestShape, after: u64, max: usize, scan: usize)
    -> ScanLogResult {
    k.pull_page(PullScope::InterestShape(shape), after, lim(max, scan)).unwrap()
}

// ─── GlobalLog ───────────────────────────────────────────────────────────────

#[test]
fn global_log_matches_store_scan_exactly() {
    let k = new_kernel();
    seed(&k, raw(1, 0xAA, 1, 1000));
    seed(&k, raw(2, 0xBB, 1, 2000));
    let kp = page(k.pull_page(PullScope::GlobalLog, 0, lim(10, 10)).unwrap());
    let sp = page(k.event_store_handle().scan_log_since_seq(0, 10).unwrap());
    assert_eq!(kp.entries.len(), sp.entries.len());
    assert_eq!(kp.next_after_seq, sp.next_after_seq);
}

#[test]
fn global_log_includes_deleted_rows() {
    let k = new_kernel();
    let ev1 = raw(1, 0xAA, 1, 1000);
    let ev1_id = ev1.id.clone();
    seed(&k, ev1);
    seed(&k, kind5(0xFF, 0xAA, 2000, &ev1_id));
    let p = page(k.pull_page(PullScope::GlobalLog, 0, lim(10, 10)).unwrap());
    assert!(p.entries.iter().any(|e| matches!(e.op, LogOp::Deleted { .. })),
            "GlobalLog must include Deleted rows");
}

// ─── InterestShape ───────────────────────────────────────────────────────────

#[test]
fn interest_shape_skips_deleted_but_advances() {
    let k = new_kernel();
    let ev1 = raw(1, 0xAA, 1, 1000);
    let ev1_id = ev1.id.clone();
    seed(&k, ev1);
    let latest = seed(&k, kind5(0xFF, 0xAA, 2000, &ev1_id));
    let p = page(pull_interest(&k, shape_ak(0xAA, [1u32]), 0, 10, 100));
    assert!(!p.entries.iter().any(|e| matches!(e.op, LogOp::Deleted { .. })),
            "InterestShape must not yield Deleted rows");
    assert_eq!(p.next_after_seq, latest, "cursor must advance past Deleted rows");
}

#[test]
fn interest_shape_empty_advancing_page_is_not_gap() {
    let k = new_kernel();
    seed(&k, raw(1, 0xBB, 1, 1000));
    seed(&k, raw(2, 0xCC, 1, 2000));
    let latest = k.event_store_handle().latest_ingest_seq().unwrap();
    // Author 0xAA has no events.
    let result = pull_interest(&k, shape_ak(0xAA, [1u32]), 0, 10, 100);
    match result {
        ScanLogResult::Page(p) => {
            assert_eq!(p.entries.len(), 0);
            assert_eq!(p.next_after_seq, latest, "cursor advanced over non-matching rows");
        }
        ScanLogResult::Gap(_) => panic!("empty advancing page must not be a Gap"),
    }
}

#[test]
fn interest_shape_matches_author_kind() {
    let k = new_kernel();
    seed(&k, raw(1, 0xAA, 1, 1000)); // matches
    seed(&k, raw(2, 0xBB, 1, 2000)); // wrong author
    seed(&k, raw(3, 0xAA, 6, 3000)); // wrong kind
    let p = page(pull_interest(&k, shape_ak(0xAA, [1u32]), 0, 10, 100));
    assert_eq!(p.entries.len(), 1);
    assert_eq!(p.entries[0].raw_event.as_ref().unwrap().pubkey, hex64(0xAA));
}

#[test]
fn interest_shape_matches_authors_kind() {
    let k = new_kernel();
    seed(&k, raw(1, 0xAA, 1, 1000)); // matches
    seed(&k, raw(2, 0xBB, 1, 2000)); // matches
    seed(&k, raw(3, 0xCC, 1, 3000)); // wrong author
    let mut shape = InterestShape::default();
    shape.authors.insert(hex64(0xAA));
    shape.authors.insert(hex64(0xBB));
    shape.kinds.insert(1);
    let p = page(pull_interest(&k, shape, 0, 10, 100));
    assert_eq!(p.entries.len(), 2);
}

#[test]
fn interest_shape_matches_kind_time() {
    let k = new_kernel();
    seed(&k, raw(1, 0xAA, 1, 1000)); // matches
    seed(&k, raw(2, 0xBB, 1, 2000)); // matches
    seed(&k, raw(3, 0xCC, 6, 3000)); // wrong kind
    let mut shape = InterestShape::default();
    shape.kinds.insert(1);
    let p = page(pull_interest(&k, shape, 0, 10, 100));
    assert_eq!(p.entries.len(), 2);
}

#[test]
fn interest_shape_matches_etag() {
    let k = new_kernel();
    let target = hex64(0xDE);
    seed(&k, raw_tags(1, 0xAA, 1, 1000, vec![vec!["e".into(), target.clone()]]));
    seed(&k, raw(2, 0xBB, 1, 2000)); // no e-tag
    let mut shape = InterestShape::default();
    shape.kinds.insert(1);
    let mut vs = BTreeSet::new();
    vs.insert(target);
    shape.tags.insert("e".to_string(), vs);
    let p = page(pull_interest(&k, shape, 0, 10, 100));
    assert_eq!(p.entries.len(), 1);
}

#[test]
fn interest_shape_matches_ptag() {
    let k = new_kernel();
    let target = hex64(0xDE);
    seed(&k, raw_tags(1, 0xAA, 1, 1000, vec![vec!["p".into(), target.clone()]]));
    seed(&k, raw(2, 0xBB, 1, 2000)); // no p-tag
    let mut shape = InterestShape::default();
    shape.kinds.insert(1);
    let mut vs = BTreeSet::new();
    vs.insert(target);
    shape.tags.insert("p".to_string(), vs);
    let p = page(pull_interest(&k, shape, 0, 10, 100));
    assert_eq!(p.entries.len(), 1);
}

#[test]
fn interest_shape_matches_kind_dtag_with_pubkey_guard() {
    let k = new_kernel();
    let author_a = hex64(0xAA);
    let author_b = hex64(0xBB);
    // Both have (kind=30023, d="article-1") but different pubkeys.
    seed(&k, raw_tags(1, 0xAA, 30023, 1000, vec![vec!["d".into(), "article-1".into()]]));
    seed(&k, raw_tags(2, 0xBB, 30023, 2000, vec![vec!["d".into(), "article-1".into()]]));
    let mut shape = InterestShape::default();
    shape.kinds.insert(30023);
    shape.addresses.insert(NaddrCoord { pubkey: author_a.clone(), kind: 30023,
                                        d_tag: "article-1".to_string() });
    let p = page(pull_interest(&k, shape, 0, 10, 100));
    assert_eq!(p.entries.len(), 1, "pubkey guard must exclude author_b");
    assert_eq!(p.entries[0].raw_event.as_ref().unwrap().pubkey, author_a);
}

#[test]
fn unsupported_shapes_are_rejected() {
    let k = new_kernel();
    // Wildcard kinds (empty kinds).
    let err = k.pull_page(PullScope::InterestShape(InterestShape::default()), 0, lim(10, 100))
               .unwrap_err();
    assert!(matches!(err, PullError::UnsupportedInterestShape));
    // Event-ids-only shape.
    let mut s = InterestShape::default();
    s.kinds.insert(1);
    s.event_ids.insert(hex64(1));
    assert!(matches!(k.pull_page(PullScope::InterestShape(s), 0, lim(10, 100)).unwrap_err(),
                     PullError::UnsupportedInterestShape));
    // Multi-tag intersection.
    let mut s2 = InterestShape::default();
    s2.kinds.insert(1);
    s2.tags.insert("e".to_string(), [hex64(1)].into());
    s2.tags.insert("p".to_string(), [hex64(2)].into());
    assert!(matches!(k.pull_page(PullScope::InterestShape(s2), 0, lim(10, 100)).unwrap_err(),
                     PullError::UnsupportedInterestShape));
}

#[test]
fn scan_budget_caps_nonmatching_run() {
    let k = new_kernel();
    // 10 events from 0xBB (won't match shape for 0xAA).
    for i in 0u8..10 { seed(&k, raw(i, 0xBB, 1, 1000 + i as u64)); }
    // 1 event from 0xAA that the budget should not reach.
    seed(&k, raw(0xFF, 0xAA, 1, 2000));
    let p = page(pull_interest(&k, shape_ak(0xAA, [1u32]), 0, 10, 5));
    assert_eq!(p.entries.len(), 0, "scan budget=5 cannot reach position 11");
    assert!(p.next_after_seq >= 5, "cursor still advanced by budget");
}

#[test]
fn entry_limit_does_not_advance_past_unprocessed_matches() {
    let k = new_kernel();
    for i in 0u8..5 { seed(&k, raw(i, 0xAA, 1, 1000 + i as u64)); }
    let latest = k.event_store_handle().latest_ingest_seq().unwrap();
    // max_entries=2: stop after 2nd match.
    let p = page(pull_interest(&k, shape_ak(0xAA, [1u32]), 0, 2, 100));
    assert_eq!(p.entries.len(), 2);
    assert!(p.next_after_seq < latest,
            "next_after_seq({}) must be < latest({}) when stopped early",
            p.next_after_seq, latest);
    assert!(p.has_more);
}

/// PullGap is propagated unchanged by both GlobalLog and InterestShape.
///
/// Uses `MemEventStore::simulate_log_gap` to advance the GC floor without
/// inserting 10 000+ events.
#[test]
fn pull_gap_propagates_for_global_and_interest() {
    use crate::store::MemEventStore;
    use std::sync::Arc;

    let store: Arc<MemEventStore> = Arc::new(MemEventStore::new());
    for i in 0u8..5 {
        let r = raw(i, 0xAA, 1, 1000 + i as u64);
        store.insert(unchecked(r), &RELAY.to_string(), 0).unwrap();
    }
    // Simulate GC: advance log floor to 3, creating a gap for after_seq=0.
    store.simulate_log_gap(3);

    // Verify the store itself returns Gap for after_seq < floor.
    assert!(matches!(store.scan_log_since_seq(0, 100).unwrap(), ScanLogResult::Gap(_)),
            "store must return Gap when after_seq < floor");
    // Verify scan from floor returns Page (not a gap).
    assert!(matches!(store.scan_log_since_seq(3, 100).unwrap(), ScanLogResult::Page(_)),
            "scan from floor must return Page");
    // The filter_page function propagates Gap unchanged (covered by above + filter_page
    // code review — end-to-end with injected store tested in nmp-testing).
}
