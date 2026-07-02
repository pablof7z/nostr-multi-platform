//! ADR-0072 §6 step-4 — Protected-cursor log-retention tests (`MemEventStore`).
//!
//! Exercises the append-time trim rule directly (white-box) so the
//! `DEFAULT_LOG_MAX_ENTRIES` (10_000) normal floor can be crossed without
//! inserting tens of thousands of events: the log is seeded sparsely with a
//! high `ingest_seq` and `log_gc_trim` is invoked under the state lock.
//!
//! Coverage:
//!   - a Protected claim holds the floor below the normal floor;
//!   - a claim whose lag exceeds `max_lag_entries` is dropped → normal trim →
//!     a scan from the old `after_seq` returns an explicit `PullGap`;
//!   - multiple Protected claims → the min eligible `after_seq` wins;
//!   - GapAllowed cursors publish nothing (empty set) → normal trim;
//!   - a stuck Protected cursor cannot grow the log past the worst-case bound;
//!   - `replace_log_retention_claims` stores the volatile set.

use crate::events::EventStore;
use crate::ingest_log::{
    LogOp, LogRetentionClaim, ScanLogResult, StoreLogEntry, DEFAULT_LOG_MAX_ENTRIES,
};
use crate::mem::ingest_log::log_gc_trim;
use crate::mem::MemEventStore;

/// A minimal log row at `seq` (trim keys on seq only; the value is opaque).
fn entry(seq: u64) -> StoreLogEntry {
    StoreLogEntry {
        seq,
        op: LogOp::Inserted,
        event_id: [0u8; 32],
        raw_event: None,
        source_relay: None,
        received_at_ms: 0,
    }
}

/// Seed sparse log state: set `ingest_seq` / `log_gc_floor` and populate the
/// given seqs, then install `claims`.
fn seed(
    store: &MemEventStore,
    latest: u64,
    floor: u64,
    seqs: &[u64],
    claims: Vec<LogRetentionClaim>,
) {
    let mut st = store.state.lock().unwrap();
    st.ingest_seq = latest;
    st.log_gc_floor = floor;
    st.ingest_log.clear();
    for &s in seqs {
        st.ingest_log.insert(s, entry(s));
    }
    st.retention_claims = claims;
}

#[test]
fn protected_claim_holds_floor_below_normal() {
    let store = MemEventStore::new();
    // latest=20_000 → normal_floor=10_000. Protected after_seq=9_000, lag
    // 11_000 <= 12_000 → eligible → floor capped at 9_000 (below normal).
    seed(
        &store,
        20_000,
        0,
        &[5_000, 9_000, 15_000, 19_999],
        vec![LogRetentionClaim {
            after_seq: 9_000,
            max_lag_entries: 12_000,
        }],
    );
    let mut st = store.state.lock().unwrap();
    log_gc_trim(&mut st);
    assert_eq!(
        st.log_gc_floor, 9_000,
        "floor pinned to protected after_seq, not the normal 10_000"
    );
    assert!(
        st.ingest_log.contains_key(&15_000),
        "unconsumed row retained"
    );
    assert!(
        st.ingest_log.contains_key(&19_999),
        "unconsumed row retained"
    );
    assert!(!st.ingest_log.contains_key(&9_000), "consumed row pruned");
    assert!(!st.ingest_log.contains_key(&5_000), "consumed row pruned");
}

#[test]
fn lag_exceeds_bound_drops_claim_then_scan_returns_gap() {
    let store = MemEventStore::new();
    // lag = 20_000 - 9_000 = 11_000 > 10_000 → claim filtered out → normal
    // floor (10_000) advances past the protected cursor.
    seed(
        &store,
        20_000,
        0,
        &[9_000, 10_500, 15_000],
        vec![LogRetentionClaim {
            after_seq: 9_000,
            max_lag_entries: 10_000,
        }],
    );
    {
        let mut st = store.state.lock().unwrap();
        log_gc_trim(&mut st);
        assert_eq!(st.log_gc_floor, 10_000, "dropped claim → normal trim");
        assert!(!st.ingest_log.contains_key(&9_000));
        assert!(st.ingest_log.contains_key(&10_500));
        assert!(st.ingest_log.contains_key(&15_000));
    }
    // The stale protected cursor now scans from its old after_seq (9_000),
    // which is below the floor → explicit PullGap, never a silent skip.
    match store.scan_log_since_seq(9_000, 100).unwrap() {
        ScanLogResult::Gap(g) => {
            assert_eq!(g.requested_after_seq, 9_000);
            assert_eq!(g.first_available_seq, 10_001, "first_available = floor + 1");
        }
        ScanLogResult::Page(_) => panic!("expected Gap after claim dropped"),
    }
}

#[test]
fn multiple_protected_min_after_seq_wins() {
    let store = MemEventStore::new();
    // Two eligible claims; the slowest (min after_seq = 9_000) sets the cap.
    seed(
        &store,
        20_000,
        0,
        &[8_000, 9_500, 13_000],
        vec![
            LogRetentionClaim {
                after_seq: 12_000,
                max_lag_entries: 20_000,
            },
            LogRetentionClaim {
                after_seq: 9_000,
                max_lag_entries: 20_000,
            },
        ],
    );
    let mut st = store.state.lock().unwrap();
    log_gc_trim(&mut st);
    assert_eq!(st.log_gc_floor, 9_000, "min eligible after_seq wins");
    assert!(
        st.ingest_log.contains_key(&9_500),
        "row above slowest cursor kept"
    );
    assert!(st.ingest_log.contains_key(&13_000));
}

#[test]
fn gap_allowed_cursors_never_pin() {
    let store = MemEventStore::new();
    // GapAllowed cursors publish NO claim → empty set → normal floor.
    seed(&store, 20_000, 0, &[5_000, 12_000], Vec::new());
    let mut st = store.state.lock().unwrap();
    log_gc_trim(&mut st);
    assert_eq!(st.log_gc_floor, 10_000, "empty claims → normal trim");
    assert!(!st.ingest_log.contains_key(&5_000));
    assert!(st.ingest_log.contains_key(&12_000));
}

#[test]
fn stuck_protected_cursor_log_stays_bounded() {
    let store = MemEventStore::new();
    // A protected cursor stuck at after_seq=0 with a small bound. Once latest
    // outruns max_lag_entries the claim is dropped and the floor advances
    // normally — the retained span can never exceed
    // max(DEFAULT_LOG_MAX_ENTRIES, max_lag_entries).
    let max_lag = 5_000u64;
    seed(
        &store,
        20_000,
        0,
        &[1, 9_000, 15_000],
        vec![LogRetentionClaim {
            after_seq: 0,
            max_lag_entries: max_lag,
        }],
    );
    let mut st = store.state.lock().unwrap();
    log_gc_trim(&mut st);
    assert_eq!(
        st.log_gc_floor, 10_000,
        "stuck claim dropped → normal floor"
    );
    let retained_span = st.ingest_seq - st.log_gc_floor;
    assert!(
        retained_span <= DEFAULT_LOG_MAX_ENTRIES.max(max_lag),
        "retained span {retained_span} exceeds worst-case bound"
    );
}

#[test]
fn replace_log_retention_claims_stores_volatile_set() {
    let store = MemEventStore::new();
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
    assert_eq!(store.state.lock().unwrap().retention_claims, claims);

    // Wholesale replace with the empty set clears it.
    store.replace_log_retention_claims(&[]);
    assert!(store.state.lock().unwrap().retention_claims.is_empty());
}
