//! Fix-verification tests for `MemEventStore` ingest log (500-LOC cap split
//! from `ingest_log_tests.rs`).
//!
//! Covers:
//!   - BLOCKING 1: Duplicate kind:5 → no new seq, no new log row
//!   - BLOCKING 2: NIP-40 expiry → Deleted{Nip40Expiry} log row emitted by gc_step
//!   - BLOCKING 3: kind:5 a-tag regular-replaceable → target removed + Deleted{Nip09}

use crate::events::EventStore;
use crate::ingest_log::{DeleteReason, LogOp, ScanLogResult};
use crate::mem::MemEventStore;
use crate::types::{GcBudget, RawEvent, VerifiedEvent};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn unchecked(raw: RawEvent) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(raw)
}

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

/// Build a kind:5 that a-tags `addr` (e.g. "0:<pk>:" for regular-replaceable).
fn make_kind5_atag(id_byte: u8, pubkey_byte: u8, created_at: u64, addr: String) -> RawEvent {
    RawEvent {
        id: format!("{:02x}", id_byte).repeat(32),
        pubkey: format!("{:02x}", pubkey_byte).repeat(32),
        created_at,
        kind: 5,
        tags: vec![vec!["a".into(), addr]],
        content: String::new(),
        sig: "a".repeat(128),
    }
}

/// Build an event with an `expiration` tag set to `expire_at` Unix seconds.
fn make_expiring_event(id_byte: u8, created_at: u64, expire_at: u64) -> RawEvent {
    RawEvent {
        id: format!("{:02x}", id_byte).repeat(32),
        pubkey: "01".repeat(32),
        created_at,
        kind: 1,
        tags: vec![vec!["expiration".into(), expire_at.to_string()]],
        content: String::new(),
        sig: "a".repeat(128),
    }
}

const RELAY: &str = "wss://test/";

// ── BLOCKING 1: duplicate kind:5 ─────────────────────────────────────────────

/// Re-delivering a kind:5 MUST NOT allocate a new seq or add any log row.
/// Parity: lmdb/insert_kind5.rs (`has_event` gate).
#[test]
fn kind5_duplicate_emits_no_new_seq_or_log_row() {
    let store = MemEventStore::new();
    let target = make_event(0x20, 500);
    store
        .insert(unchecked(target.clone()), &RELAY.to_string(), 500_000)
        .unwrap();

    let k5 = make_kind5(0x21, 0x01, 600, target.id.clone());
    store
        .insert(unchecked(k5.clone()), &RELAY.to_string(), 600_000)
        .unwrap();
    let seq_after_first = store.latest_ingest_seq().unwrap();
    let log_len_after_first = store.state.lock().unwrap().ingest_log.len();

    // Re-deliver the identical kind:5.
    store
        .insert(unchecked(k5), &RELAY.to_string(), 601_000)
        .unwrap();

    assert_eq!(
        store.latest_ingest_seq().unwrap(),
        seq_after_first,
        "BLOCKING 1: duplicate kind:5 must NOT allocate a new seq"
    );
    assert_eq!(
        store.state.lock().unwrap().ingest_log.len(),
        log_len_after_first,
        "BLOCKING 1: duplicate kind:5 must NOT add a new log row"
    );
}

// ── BLOCKING 2: NIP-40 expiry emits Deleted{Nip40Expiry} ────────────────────

/// gc_step Phase 1 (NIP-40 expiry reap) MUST emit exactly one
/// `Deleted{Nip40Expiry}` log row per reaped event.
/// Parity: lmdb/gc.rs (expiry loop).
#[test]
fn nip40_expiry_emits_deleted_nip40expiry() {
    let store = MemEventStore::new();

    let ev = make_expiring_event(0x30, 500, 1000);
    let target_id = ev.id_bytes().unwrap();
    store
        .insert(unchecked(ev), &RELAY.to_string(), 500_000)
        .unwrap();
    let seq_after_insert = store.latest_ingest_seq().unwrap();

    let budget = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 10_000,
        max_total_events: usize::MAX,
    };
    store.gc_step(budget, 2000).unwrap();

    let result = store.scan_log_since_seq(seq_after_insert, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            let expiry_entries: Vec<_> = page
                .entries
                .iter()
                .filter(|e| {
                    matches!(
                        &e.op,
                        LogOp::Deleted {
                            reason: DeleteReason::Nip40Expiry,
                            target_id: tid
                        } if *tid == target_id
                    )
                })
                .collect();
            assert_eq!(
                expiry_entries.len(),
                1,
                "BLOCKING 2: must emit exactly one Deleted{{Nip40Expiry}} per reaped event"
            );
        }
        ScanLogResult::Gap(_) => panic!("expected Page"),
    }
}

// ── BLOCKING 3: a-tag regular-replaceable ────────────────────────────────────

/// A kind:5 with an a-tag targeting a regular-replaceable (kind:0, empty d-tag,
/// addr = "0:<pubkey>:") MUST remove the target and emit Deleted{Nip09}.
/// Parity: lmdb/insert_kind5.rs (is_replaceable branch).
#[test]
fn kind5_atag_regular_replaceable_removes_target_and_logs() {
    let store = MemEventStore::new();

    let pk_hex = "05".repeat(32);
    let target = RawEvent {
        id: "aa".repeat(32),
        pubkey: pk_hex.clone(),
        created_at: 100,
        kind: 0,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };
    let target_id = target.id_bytes().unwrap();
    store
        .insert(unchecked(target), &RELAY.to_string(), 100_000)
        .unwrap();

    let addr = format!("0:{pk_hex}:");
    let k5 = make_kind5_atag(0x22, 0x05, 200, addr);
    let k5_id = k5.id_bytes().unwrap();
    store
        .insert(unchecked(k5), &RELAY.to_string(), 200_000)
        .unwrap();

    assert!(
        !store
            .state
            .lock()
            .unwrap()
            .events
            .contains_key(&"aa".repeat(32)),
        "BLOCKING 3: kind:0 regular-replaceable target must be removed by kind:5 a-tag"
    );

    match store.scan_log_since_seq(0, 100).unwrap() {
        ScanLogResult::Page(page) => {
            let deleted_entries: Vec<_> = page
                .entries
                .iter()
                .filter(|e| {
                    matches!(
                        &e.op,
                        LogOp::Deleted { reason: DeleteReason::Nip09, target_id: tid }
                        if *tid == target_id
                    )
                })
                .collect();
            assert_eq!(
                deleted_entries.len(),
                1,
                "BLOCKING 3: must emit exactly one Deleted{{Nip09}} for regular-replaceable target"
            );
            assert_eq!(
                deleted_entries[0].event_id, k5_id,
                "carrier event_id must be the kind:5 id"
            );
        }
        ScanLogResult::Gap(_) => panic!("expected Page"),
    }
}
