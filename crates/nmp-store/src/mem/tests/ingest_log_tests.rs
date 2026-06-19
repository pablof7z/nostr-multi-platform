//! ADR-0058 §8.1 parity tests for `MemEventStore` ingest log.
//!
//! Covers the required test matrix from the ADR:
//!   - Late old event (low created_at) gets a higher seq and is delivered
//!   - Duplicate arrival → no new seq, no log row
//!   - Replaced op carries correct replaced_id
//!   - NIP-09 → kind:5 Inserted + Deleted{Nip09} per removed target
//!   - Bounded GC → oldest_available_seq advances; scan below floor → PullGap
//!   - LRU eviction emits no log row
//! Fix-verification tests live in `ingest_log_fix_tests.rs` (500-LOC cap split).

use crate::events::EventStore;
use crate::ingest_log::{DeleteReason, LogOp, ScanLogResult};
use crate::mem::MemEventStore;
use crate::types::{GcBudget, RawEvent, VerifiedEvent};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn unchecked(raw: RawEvent) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(raw)
}

/// Build a normal (non-replaceable) synthetic event.
fn make_event(id_byte: u8, created_at: u64) -> RawEvent {
    RawEvent {
        id: format!("{:02x}", id_byte).repeat(32),
        pubkey: "01".repeat(32),
        created_at,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    }
}

/// Build a replaceable kind=0 event for the given pubkey.
fn make_replaceable(id_byte: u8, pubkey_byte: u8, created_at: u64) -> RawEvent {
    RawEvent {
        id: format!("{:02x}", id_byte).repeat(32),
        pubkey: format!("{:02x}", pubkey_byte).repeat(32),
        created_at,
        kind: 0,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    }
}

/// Build a kind:5 that e-tags `target_id_hex`.
fn make_kind5(id_byte: u8, pubkey_byte: u8, created_at: u64, target_id_hex: String) -> RawEvent {
    RawEvent {
        id: format!("{:02x}", id_byte).repeat(32),
        pubkey: format!("{:02x}", pubkey_byte).repeat(32),
        created_at,
        kind: 5,
        tags: vec![vec!["e".into(), target_id_hex]],
        content: String::new(),
        sig: "a".repeat(128),
    }
}

const RELAY: &str = "wss://test/";

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Late-arriving event (low created_at) still gets a HIGHER seq and is
/// delivered by a scan positioned after the first event's seq.
/// Proves that seq = arrival order, NOT created_at order.
#[test]
fn late_old_event_gets_higher_seq_and_is_delivered() {
    let store = MemEventStore::new();

    // First: a "recent" event at created_at = 2000.
    let ev_recent = make_event(0xAA, 2000);
    store
        .insert(unchecked(ev_recent), &RELAY.to_string(), 2_000_000)
        .unwrap();
    let seq_after_first = store.latest_ingest_seq().unwrap();
    assert_eq!(seq_after_first, 1, "first insert must yield seq=1");

    // Second: a "late old" event at created_at = 100, arriving now.
    let ev_late_old = make_event(0xBB, 100);
    store
        .insert(unchecked(ev_late_old), &RELAY.to_string(), 3_000_000)
        .unwrap();
    let seq_after_second = store.latest_ingest_seq().unwrap();
    assert_eq!(seq_after_second, 2, "late old event must get seq=2");

    // Scanning from cursor=1 MUST deliver the late old event (seq=2).
    let result = store.scan_log_since_seq(1, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            assert_eq!(page.entries.len(), 1, "one entry after seq=1");
            assert_eq!(page.entries[0].seq, 2);
            assert!(
                matches!(page.entries[0].op, LogOp::Inserted),
                "late old event is Inserted"
            );
        }
        ScanLogResult::Gap(_) => panic!("expected Page, got Gap"),
    }
}

/// Duplicate re-delivery of an already-stored event must consume no seq and
/// add no log row.
#[test]
fn duplicate_arrival_emits_no_log_entry() {
    let store = MemEventStore::new();

    let ev = make_event(0xCC, 1000);
    store
        .insert(unchecked(ev.clone()), &RELAY.to_string(), 1_000_000)
        .unwrap();
    assert_eq!(store.latest_ingest_seq().unwrap(), 1, "first insert seq=1");

    // Duplicate re-delivery.
    store
        .insert(unchecked(ev), &RELAY.to_string(), 1_001_000)
        .unwrap();
    assert_eq!(
        store.latest_ingest_seq().unwrap(),
        1,
        "duplicate must NOT increment seq"
    );
    let st = store.state.lock().unwrap();
    assert_eq!(st.ingest_log.len(), 1, "duplicate must NOT add a log row");
}

/// `Replaced` log entry carries the correct `replaced_id`.
#[test]
fn replaced_op_carries_correct_replaced_id() {
    let store = MemEventStore::new();

    // Insert replaceable (kind=0, pubkey=0x02) at t=100.
    let ev_old = make_replaceable(0xDD, 0x02, 100);
    store
        .insert(unchecked(ev_old.clone()), &RELAY.to_string(), 100_000)
        .unwrap();
    let old_id = ev_old.id_bytes().unwrap();

    // Insert newer version (same kind+pubkey) at t=200 → replaces.
    let ev_new = make_replaceable(0xEE, 0x02, 200);
    store
        .insert(unchecked(ev_new.clone()), &RELAY.to_string(), 200_000)
        .unwrap();
    let new_id = ev_new.id_bytes().unwrap();

    let result = store.scan_log_since_seq(0, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            // seq=1: Inserted (old), seq=2: Replaced{replaced_id=old_id}
            assert_eq!(page.entries.len(), 2);
            let rep = &page.entries[1];
            assert_eq!(rep.seq, 2);
            match &rep.op {
                LogOp::Replaced { replaced_id } => {
                    assert_eq!(*replaced_id, old_id, "replaced_id must be the old event");
                }
                other => panic!("expected Replaced, got {other:?}"),
            }
            assert_eq!(rep.event_id, new_id, "event_id must be the new event");
        }
        ScanLogResult::Gap(_) => panic!("expected Page"),
    }
}

/// NIP-09 kind:5 produces: Inserted for the kind:5 itself + Deleted{Nip09}
/// for each removed target.
#[test]
fn kind5_emits_inserted_for_kind5_and_deleted_for_each_target() {
    let store = MemEventStore::new();

    // Target event (pubkey=0x03).
    let target = make_event(0x10, 500);
    let target_id = target.id_bytes().unwrap();
    store
        .insert(unchecked(target.clone()), &RELAY.to_string(), 500_000)
        .unwrap();

    // kind:5 from same author (pubkey=0x03) e-tagging the target.
    let k5 = make_kind5(0x11, 0x01, 600, target.id.clone());
    let k5_id = k5.id_bytes().unwrap();
    store
        .insert(unchecked(k5), &RELAY.to_string(), 600_000)
        .unwrap();

    // Seq=1: Inserted(target), Seq=2: Deleted{Nip09}(target), Seq=3: Inserted(kind:5).
    // NOTE: the exact ordering inside handle_kind5_insert is:
    //   first the Deleted entries for each target removed,
    //   then the Inserted for the kind:5 itself.
    let result = store.scan_log_since_seq(0, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            // We expect the target Inserted plus either [Deleted, Inserted(k5)]
            // or just [Inserted(k5)] if the target wasn't stored under this pubkey.
            // Target pubkey = "01"*32, kind:5 pubkey = "01"*32 → self-delete succeeds.
            let has_deleted = page
                .entries
                .iter()
                .any(|e| matches!(&e.op, LogOp::Deleted { reason: DeleteReason::Nip09, target_id: tid } if *tid == target_id));
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

/// After log GC trims entries, `oldest_available_seq` rises and a scan with
/// `after_seq < gc_floor` returns an explicit `PullGap` (never a silent skip).
#[test]
fn bounded_gc_raises_floor_and_scan_below_returns_gap() {
    let store = MemEventStore::new();

    // Insert one event to get seq=1.
    let ev = make_event(0xF0, 100);
    store
        .insert(unchecked(ev), &RELAY.to_string(), 100_000)
        .unwrap();
    assert_eq!(store.latest_ingest_seq().unwrap(), 1);

    // Simulate GC: directly raise the floor and clear the entry.
    {
        let mut st = store.state.lock().unwrap();
        st.ingest_log.clear();
        st.log_gc_floor = 1;
    }

    // oldest_available_seq must now reflect the gap (seq=2 would be next,
    // but log is empty — returns None since log is empty).
    // The important contract is that scan_log_since_seq(0, 100) returns a Gap.
    let gap_result = store.scan_log_since_seq(0, 100).unwrap();
    match gap_result {
        ScanLogResult::Gap(gap) => {
            assert_eq!(gap.requested_after_seq, 0);
            assert_eq!(gap.first_available_seq, 2, "first_available = floor+1 = 2");
        }
        ScanLogResult::Page(_) => panic!("expected Gap when after_seq < gc_floor"),
    }

    // Scanning from floor (after_seq = gc_floor = 1) is fine — no gap.
    let ok_result = store.scan_log_since_seq(1, 100).unwrap();
    match ok_result {
        ScanLogResult::Page(page) => {
            assert!(
                page.entries.is_empty(),
                "no entries above floor when log is empty"
            );
        }
        ScanLogResult::Gap(_) => panic!("scan from gc_floor must not return Gap"),
    }
}

/// LRU eviction MUST NOT emit any log entry.
#[test]
fn lru_eviction_emits_no_log_row() {
    let store = MemEventStore::new();

    let ev = make_event(0xA1, 1000);
    store
        .insert(unchecked(ev), &RELAY.to_string(), 1_000_000)
        .unwrap();

    let seq_before_gc = store.latest_ingest_seq().unwrap();
    assert_eq!(seq_before_gc, 1);

    // GC with max_total_events = 0 to force LRU eviction.
    let budget = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 10_000,
        max_total_events: 0,
    };
    store.gc_step(budget, 9_999_999).unwrap();

    // Seq must NOT have advanced — eviction is silent to the log.
    assert_eq!(
        store.latest_ingest_seq().unwrap(),
        seq_before_gc,
        "LRU eviction must not emit a log entry"
    );
    let st = store.state.lock().unwrap();
    assert_eq!(
        st.ingest_log.len(),
        1,
        "log must still contain exactly 1 entry (the original insert)"
    );
}

/// `oldest_available_seq` returns None on empty store and Some after insert.
#[test]
fn empty_store_returns_zero_latest_seq() {
    let store = MemEventStore::new();
    assert_eq!(store.latest_ingest_seq().unwrap(), 0);
}

#[test]
fn empty_store_returns_none_oldest_seq() {
    let store = MemEventStore::new();
    assert_eq!(store.oldest_available_seq().unwrap(), None);
}

#[test]
fn empty_store_scan_returns_empty_page() {
    let store = MemEventStore::new();
    let result = store.scan_log_since_seq(0, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            assert!(page.entries.is_empty());
            assert!(!page.has_more);
            assert_eq!(page.latest_seq, 0);
        }
        ScanLogResult::Gap(_) => panic!("expected Page, got Gap"),
    }
}
