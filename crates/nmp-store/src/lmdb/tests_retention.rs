//! ADR-0072 §6 step-4 — Protected-cursor log-retention tests (LMDB backend).
//!
//! Mirrors the `MemEventStore` retention matrix for parity. The
//! `DEFAULT_LOG_MAX_ENTRIES` (10_000) normal floor is crossed without inserting
//! tens of thousands of events: the `nmp-ingest-meta` `last_seq` is seeded high
//! and the log is populated sparsely via the inner sub-dbs, then one real
//! `insert` drives the append-time `trim_in_txn` (inside the event `RwTxn`).
//!
//! LMDB tests run only on Linux CI; locally they are compiled with
//! `--features lmdb-backend --no-run`.

#![cfg(all(test, feature = "lmdb-backend"))]

use crate::events::EventStore;
use crate::ingest_log::{LogRetentionClaim, ScanLogResult, DEFAULT_LOG_MAX_ENTRIES};
use crate::LmdbEventStore;

use super::test_fixtures::{open_tmp, signed_event, verified};

const RELAY: &str = "wss://test/";

// ── Direct sub-db seeding helpers ───────────────────────────────────────────────

/// Seed `nmp-ingest-meta` `last_seq` / `gc_floor`.
fn write_meta(store: &LmdbEventStore, last_seq: u64, gc_floor: u64) {
    let inner = store.inner_for_test();
    let mut txn = inner.env.write_txn().unwrap();
    inner
        .ingest_meta
        .put(&mut txn, b"last_seq", &last_seq.to_be_bytes())
        .unwrap();
    inner
        .ingest_meta
        .put(&mut txn, b"gc_floor", &gc_floor.to_be_bytes())
        .unwrap();
    txn.commit().unwrap();
}

/// Insert a sparse log row at `seq` (trim keys on seq only; value is opaque).
fn put_log_row(store: &LmdbEventStore, seq: u64) {
    let inner = store.inner_for_test();
    let mut txn = inner.env.write_txn().unwrap();
    inner
        .ingest_log
        .put(&mut txn, &seq.to_be_bytes(), b"x")
        .unwrap();
    txn.commit().unwrap();
}

fn read_gc_floor(store: &LmdbEventStore) -> u64 {
    let inner = store.inner_for_test();
    let txn = inner.env.read_txn().unwrap();
    let v = inner.ingest_meta.get(&txn, b"gc_floor").unwrap().unwrap();
    u64::from_be_bytes(v[..8].try_into().unwrap())
}

fn has_row(store: &LmdbEventStore, seq: u64) -> bool {
    let inner = store.inner_for_test();
    let txn = inner.env.read_txn().unwrap();
    inner
        .ingest_log
        .get(&txn, &seq.to_be_bytes())
        .unwrap()
        .is_some()
}

/// Trigger one append-time trim by inserting a real event (seq -> last_seq+1).
fn trigger_trim(store: &LmdbEventStore) {
    store
        .insert(
            verified(signed_event(1, 100, "trim", None)),
            &RELAY.into(),
            1,
        )
        .unwrap();
}

// ── Tests ───────────────────────────────────────────────────────────────────────

#[test]
fn protected_claim_holds_floor_below_normal() {
    let (store, _dir) = open_tmp();
    write_meta(&store, 20_000, 0);
    for s in [5_000u64, 9_000, 15_000, 19_999] {
        put_log_row(&store, s);
    }
    // latest after insert = 20_001 → normal_floor = 10_001. Protected after_seq
    // 9_000, lag 11_001 <= 12_000 → eligible → floor capped at 9_000.
    store.replace_log_retention_claims(&[LogRetentionClaim {
        after_seq: 9_000,
        max_lag_entries: 12_000,
    }]);
    trigger_trim(&store);

    assert_eq!(
        read_gc_floor(&store),
        9_000,
        "floor pinned to protected after_seq"
    );
    assert!(has_row(&store, 15_000), "unconsumed row retained");
    assert!(has_row(&store, 19_999), "unconsumed row retained");
    assert!(!has_row(&store, 9_000), "consumed row pruned");
    assert!(!has_row(&store, 5_000), "consumed row pruned");
}

#[test]
fn lag_exceeds_bound_drops_claim_then_scan_returns_gap() {
    let (store, _dir) = open_tmp();
    write_meta(&store, 20_000, 0);
    for s in [9_000u64, 10_500, 15_000] {
        put_log_row(&store, s);
    }
    // lag 11_001 > 10_000 → claim filtered out → normal floor (10_001).
    store.replace_log_retention_claims(&[LogRetentionClaim {
        after_seq: 9_000,
        max_lag_entries: 10_000,
    }]);
    trigger_trim(&store);

    assert_eq!(read_gc_floor(&store), 10_001, "dropped claim → normal trim");
    assert!(!has_row(&store, 9_000));
    assert!(has_row(&store, 10_500));

    match store.scan_log_since_seq(9_000, 100).unwrap() {
        ScanLogResult::Gap(g) => {
            assert_eq!(g.requested_after_seq, 9_000);
            assert_eq!(g.first_available_seq, 10_002, "first_available = floor + 1");
        }
        ScanLogResult::Page(_) => panic!("expected Gap after claim dropped"),
    }
}

#[test]
fn multiple_protected_min_after_seq_wins() {
    let (store, _dir) = open_tmp();
    write_meta(&store, 20_000, 0);
    for s in [8_000u64, 9_500, 13_000] {
        put_log_row(&store, s);
    }
    store.replace_log_retention_claims(&[
        LogRetentionClaim {
            after_seq: 12_000,
            max_lag_entries: 20_000,
        },
        LogRetentionClaim {
            after_seq: 9_000,
            max_lag_entries: 20_000,
        },
    ]);
    trigger_trim(&store);

    assert_eq!(read_gc_floor(&store), 9_000, "min eligible after_seq wins");
    assert!(has_row(&store, 9_500), "row above slowest cursor kept");
    assert!(has_row(&store, 13_000));
}

#[test]
fn gap_allowed_cursors_never_pin() {
    let (store, _dir) = open_tmp();
    write_meta(&store, 20_000, 0);
    for s in [5_000u64, 12_000] {
        put_log_row(&store, s);
    }
    // GapAllowed cursors publish NO claim → empty set → normal floor.
    store.replace_log_retention_claims(&[]);
    trigger_trim(&store);

    assert_eq!(read_gc_floor(&store), 10_001, "empty claims → normal trim");
    assert!(!has_row(&store, 5_000));
    assert!(has_row(&store, 12_000));
}

#[test]
fn stuck_protected_cursor_log_stays_bounded() {
    let (store, _dir) = open_tmp();
    write_meta(&store, 20_000, 0);
    for s in [1u64, 9_000, 15_000] {
        put_log_row(&store, s);
    }
    let max_lag = 5_000u64;
    // lag 20_001 > 5_000 → claim dropped → floor advances normally.
    store.replace_log_retention_claims(&[LogRetentionClaim {
        after_seq: 0,
        max_lag_entries: max_lag,
    }]);
    trigger_trim(&store);

    let floor = read_gc_floor(&store);
    assert_eq!(floor, 10_001, "stuck claim dropped → normal floor");
    let retained_span = store.latest_ingest_seq().unwrap() - floor;
    assert!(
        retained_span <= DEFAULT_LOG_MAX_ENTRIES.max(max_lag),
        "retained span {retained_span} exceeds worst-case bound"
    );
}

#[test]
fn replace_log_retention_claims_stores_volatile_set() {
    let (store, _dir) = open_tmp();
    let claims = vec![
        LogRetentionClaim {
            after_seq: 5,
            max_lag_entries: 100,
        },
        LogRetentionClaim {
            after_seq: 42,
            max_lag_entries: 9,
        },
    ];
    store.replace_log_retention_claims(&claims);
    assert_eq!(store.inner_for_test().retention_claims_snapshot(), claims);

    // Wholesale replace with the empty set clears it.
    store.replace_log_retention_claims(&[]);
    assert!(store
        .inner_for_test()
        .retention_claims_snapshot()
        .is_empty());
}
