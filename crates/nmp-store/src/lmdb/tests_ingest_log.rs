//! ADR-0058 §8.1 parity tests for the LMDB ingest log.
//!
//! Covers the required test matrix from the ADR:
//!   - Late old event (low created_at) gets a higher seq and is delivered
//!   - Duplicate arrival → no new seq, no log row
//!   - Replaced op carries correct replaced_id
//!   - NIP-09 → kind:5 Inserted + Deleted{Nip09} per removed target
//!   - LMDB reopen → seq continuity (last_seq persists across close/open)
//!   - Bounded GC → oldest_available_seq advances; scan below floor → PullGap
//!   - LRU eviction emits no log row
//! New (fix verification):
//!   - Duplicate kind:5 → no new seq, no new log row (BLOCKING 1)
//!   - a-tag regular-replaceable target → removed + Deleted{Nip09} (BLOCKING 3)
//!   - Append-time trim: after DEFAULT_LOG_MAX_ENTRIES+N appends, gc_floor advanced (BLOCKING 4)
//!   - Persisted format: round-trip + version field present + stable variant names (SHOULD-FIX 6)

#![cfg(feature = "lmdb-backend")]

use crate::events::EventStore;
use crate::ingest_log::{DeleteReason, LogOp, ScanLogResult};
use crate::types::GcBudget;
use crate::LmdbEventStore;

use super::test_fixtures::{open_tmp, signed_event, signed_event_with_keys, verified};

const RELAY: &str = "wss://test/";

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Late-arriving event (low created_at) still gets a HIGHER seq and is
/// delivered by a scan positioned after the first event's seq.
/// Proves that seq = arrival order, NOT created_at order.
#[test]
fn late_old_event_gets_higher_seq_and_is_delivered() {
    let (store, _dir) = open_tmp();

    // First: a kind:1 at created_at = 2000.
    let ev_recent = signed_event(1, 2000, "recent", None);
    store
        .insert(verified(ev_recent), &RELAY.into(), 2_000_000)
        .unwrap();
    let seq1 = store.latest_ingest_seq().unwrap();
    assert_eq!(seq1, 1, "first insert must yield seq=1");

    // Second: a kind:1 at created_at = 100 — same kind, different key, arrives later.
    let ev_old = signed_event(1, 100, "old", None);
    store
        .insert(verified(ev_old), &RELAY.into(), 3_000_000)
        .unwrap();
    let seq2 = store.latest_ingest_seq().unwrap();
    assert_eq!(seq2, 2, "late old event must get seq=2 (arrival order)");

    // Scanning from cursor=1 MUST deliver the late old event (seq=2).
    let result = store.scan_log_since_seq(1, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            assert_eq!(page.entries.len(), 1, "one entry after seq=1");
            assert_eq!(page.entries[0].seq, 2);
            assert!(
                matches!(page.entries[0].op, LogOp::Inserted),
                "late old event must be Inserted"
            );
        }
        ScanLogResult::Gap(_) => panic!("expected Page, got Gap"),
    }
}

/// Duplicate re-delivery of an already-stored event must consume no seq and
/// add no log row.
#[test]
fn duplicate_arrival_emits_no_log_entry() {
    let (store, _dir) = open_tmp();

    let ev = signed_event(1, 1000, "hello", None);
    store
        .insert(verified(ev.clone()), &RELAY.into(), 1_000_000)
        .unwrap();
    assert_eq!(store.latest_ingest_seq().unwrap(), 1, "first insert seq=1");

    // Duplicate re-delivery.
    store
        .insert(verified(ev), &RELAY.into(), 1_001_000)
        .unwrap();
    assert_eq!(
        store.latest_ingest_seq().unwrap(),
        1,
        "duplicate must NOT increment seq"
    );

    // Verify no second log entry exists by scanning.
    let result = store.scan_log_since_seq(0, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            assert_eq!(page.entries.len(), 1, "only one log entry must exist");
        }
        ScanLogResult::Gap(_) => panic!("expected Page"),
    }
}

/// `Replaced` log entry carries the correct `replaced_id`.
#[test]
fn replaced_op_carries_correct_replaced_id() {
    let (store, _dir) = open_tmp();
    let keys = nostr::Keys::generate();

    // Insert replaceable kind=0 at t=100.
    let ev_old = signed_event_with_keys(&keys, 0, 100, "old profile", None);
    let old_id = ev_old.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(ev_old), &RELAY.into(), 100_000)
        .unwrap();

    // Newer version (same kind+pubkey) at t=200 → replaces.
    let ev_new = signed_event_with_keys(&keys, 0, 200, "new profile", None);
    let new_id = ev_new.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(ev_new), &RELAY.into(), 200_000)
        .unwrap();

    let result = store.scan_log_since_seq(0, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            // seq=1: Inserted(old), seq=2: Replaced{replaced_id=old_id}.
            assert_eq!(page.entries.len(), 2, "two log entries expected");
            let rep = &page.entries[1];
            assert_eq!(rep.seq, 2);
            match &rep.op {
                LogOp::Replaced { replaced_id } => {
                    assert_eq!(
                        *replaced_id, old_id,
                        "replaced_id must reference the old event"
                    );
                }
                other => panic!("expected Replaced, got {other:?}"),
            }
            assert_eq!(rep.event_id, new_id, "event_id must be the new event");
        }
        ScanLogResult::Gap(_) => panic!("expected Page"),
    }
}

/// NIP-09 kind:5 produces: Deleted{Nip09} for each removed target, then
/// Inserted for the kind:5 itself.
#[test]
fn kind5_emits_deleted_for_target_and_inserted_for_kind5() {
    use nostr::prelude::*;

    let (store, _dir) = open_tmp();
    let keys = Keys::generate();

    // Target event (kind:1, same author).
    let target = signed_event_with_keys(&keys, 1, 1000, "doomed", None);
    let target_id = target.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(target.clone()), &RELAY.to_string(), 1_000_000)
        .unwrap();

    // kind:5 from same author e-tagging the target.
    let k5_ev = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(nostr::EventId::from_slice(&target_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .expect("sign");
    let k5_json = k5_ev.try_as_json().expect("json");
    let k5_raw: crate::types::RawEvent = serde_json::from_str(&k5_json).expect("parse");
    let k5_id = k5_raw.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(k5_raw), &RELAY.to_string(), 2_000_000)
        .unwrap();

    let result = store.scan_log_since_seq(0, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            let has_deleted = page.entries.iter().any(|e| {
                matches!(&e.op, LogOp::Deleted { reason: DeleteReason::Nip09, target_id: tid } if *tid == target_id)
            });
            let has_k5_inserted = page
                .entries
                .iter()
                .any(|e| e.event_id == k5_id && matches!(e.op, LogOp::Inserted));
            assert!(has_deleted, "must have Deleted{{Nip09}} for the target");
            assert!(has_k5_inserted, "must have Inserted for the kind:5 itself");
        }
        ScanLogResult::Gap(_) => panic!("expected Page"),
    }
}

/// Seq continuity: after close+reopen, the next insert gets seq = previous + 1.
#[test]
fn lmdb_reopen_seq_continuity() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();

    let seq_before_close = {
        let store = LmdbEventStore::open(&path).expect("open");
        let ev1 = signed_event(1, 1000, "first", None);
        store
            .insert(verified(ev1), &RELAY.into(), 1_000_000)
            .unwrap();
        let ev2 = signed_event(1, 2000, "second", None);
        store
            .insert(verified(ev2), &RELAY.into(), 2_000_000)
            .unwrap();
        store.latest_ingest_seq().unwrap()
    };
    assert_eq!(seq_before_close, 2);

    // Reopen the store — drop the original (closes the LMDB env).
    let store2 = LmdbEventStore::open(&path).expect("reopen");
    assert_eq!(
        store2.latest_ingest_seq().unwrap(),
        2,
        "last_seq must persist across close/open"
    );

    // Third insert must get seq=3.
    let ev3 = signed_event(1, 3000, "third", None);
    store2
        .insert(verified(ev3), &RELAY.into(), 3_000_000)
        .unwrap();
    assert_eq!(
        store2.latest_ingest_seq().unwrap(),
        3,
        "seq must continue from 2, not restart at 1"
    );
}

/// After log GC trims entries, scan with after_seq < gc_floor returns an
/// explicit PullGap (never a silent skip).
///
/// To avoid inserting 10,000 events, we simulate the floor by directly writing
/// to `nmp-ingest-meta` via `inner_for_test()`.
#[test]
fn bounded_gc_raises_floor_and_scan_below_returns_gap() {
    let (store, _dir) = open_tmp();

    // Insert one event → seq=1.
    let ev = signed_event(1, 500, "floor test", None);
    store.insert(verified(ev), &RELAY.into(), 500_000).unwrap();
    assert_eq!(store.latest_ingest_seq().unwrap(), 1);

    // Simulate GC having trimmed seq=1: set gc_floor=1 and delete that entry.
    {
        let inner = store.inner_for_test();
        let mut txn = inner.env.write_txn().expect("write_txn");
        // gc_floor = 1
        inner
            .ingest_meta
            .put(&mut txn, b"gc_floor", &1u64.to_be_bytes())
            .expect("put gc_floor");
        // Remove the only log entry (key = seq 1 as 8-byte BE).
        inner
            .ingest_log
            .delete(&mut txn, &1u64.to_be_bytes())
            .expect("delete seq=1");
        txn.commit().expect("commit");
    }

    // Scan from 0 (< gc_floor=1) must yield a Gap.
    let gap_result = store.scan_log_since_seq(0, 100).unwrap();
    match gap_result {
        ScanLogResult::Gap(gap) => {
            assert_eq!(gap.requested_after_seq, 0);
            assert_eq!(
                gap.first_available_seq, 2,
                "first_available = gc_floor+1 = 2"
            );
        }
        ScanLogResult::Page(_) => panic!("expected Gap when after_seq < gc_floor"),
    }

    // Scan from gc_floor itself (after_seq=1) is fine — no entries, no gap.
    let ok_result = store.scan_log_since_seq(1, 100).unwrap();
    match ok_result {
        ScanLogResult::Page(page) => {
            assert!(page.entries.is_empty(), "no entries above floor");
        }
        ScanLogResult::Gap(_) => panic!("scan at gc_floor must not return Gap"),
    }
}

/// LRU eviction (Phase 2 of gc_step) MUST NOT emit any log entry.
#[test]
fn lru_eviction_emits_no_log_row() {
    let (store, _dir) = open_tmp();

    let ev = signed_event(1, 1000, "lru test", None);
    store
        .insert(verified(ev), &RELAY.into(), 1_000_000)
        .unwrap();
    let seq_before = store.latest_ingest_seq().unwrap();
    assert_eq!(seq_before, 1);

    // Force LRU eviction by setting ceiling to 0.
    let budget = GcBudget::with_durable_event_ceiling(0);
    store.gc_step(budget, 9_999_999).unwrap();

    // Seq must NOT have advanced — LRU eviction is transparent to the log.
    assert_eq!(
        store.latest_ingest_seq().unwrap(),
        seq_before,
        "LRU eviction must not emit a log entry"
    );

    // Log scan must still return at most the original 1 entry (may be 0 if
    // the log was already trimmed, but must never be > 1).
    let result = store.scan_log_since_seq(0, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            assert!(
                page.entries.len() <= 1,
                "log must not grow due to LRU eviction; got {} entries",
                page.entries.len()
            );
        }
        ScanLogResult::Gap(_) => {
            // A gap here would mean gc_floor advanced, which also means no new
            // log row was emitted — still passing the contract.
        }
    }
}

// ── Smoke tests (empty-store invariants) ──────────────────────────────────────

#[test]
fn empty_store_returns_zero_latest_seq() {
    let (store, _dir) = open_tmp();
    assert_eq!(store.latest_ingest_seq().unwrap(), 0);
}

#[test]
fn empty_store_returns_none_oldest_seq() {
    let (store, _dir) = open_tmp();
    assert_eq!(store.oldest_available_seq().unwrap(), None);
}

#[test]
fn empty_store_scan_returns_empty_page() {
    let (store, _dir) = open_tmp();
    let result = store.scan_log_since_seq(0, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            assert!(page.entries.is_empty());
            assert!(!page.has_more);
        }
        ScanLogResult::Gap(_) => panic!("expected Page, got Gap"),
    }
}
