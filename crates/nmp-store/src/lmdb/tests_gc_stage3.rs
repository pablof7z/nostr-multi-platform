//! #1480 — production GC keeps durable rows; finite retention is explicit.
//!
//! Stage 3 (#1090) made the durable LRU deletion path safe when a caller opts
//! into a finite row ceiling. #1480 changes the production default back to
//! `usize::MAX`: the on-device GC pass reaps correctness deletes/tombstones, but
//! it does not drop valid fetched events just because they are cold. Future
//! disk/user retention policy must call an explicit finite-ceiling budget.
//!
//! These tests live in their own file (not appended to the already-baselined
//! `tests_gc.rs`, which is at the 500-LOC hard cap) per the AGENTS.md file-size
//! rule.

#![cfg(feature = "lmdb-backend")]

use std::collections::HashSet;

use crate::types::{EventId, GcBudget, InsertOutcome, DEFAULT_DURABLE_EVENT_CEILING};
use crate::EventStore;

use super::test_fixtures::{open_tmp, signed_event, verified};

/// Insert `n` distinct kind:1 events; `base_ts + i` is event `i`'s `created_at`.
fn insert_n(store: &impl EventStore, n: usize, base_ts: u64) {
    for i in 0..n {
        let raw = signed_event(1, base_ts + i as u64, &format!("event-{i}"), None);
        let outcome = store
            .insert(verified(raw), &"wss://r/".into(), 1_000_000)
            .unwrap_or_else(|e| panic!("insert #{i} failed: {e}"));
        assert!(
            matches!(outcome, InsertOutcome::Inserted { .. }),
            "expected Inserted at #{i}, got {outcome:?}",
        );
    }
}

/// #1480 — `GcBudget::production()` leaves durable LRU deletion disabled.
/// The on-device 60s GC pass must not delete valid fetched events by default;
/// RAM working-set pressure is handled by the kernel RAM-cache pass instead.
#[test]
fn gc_production_budget_keeps_durable_lru_disabled() {
    assert_eq!(
        GcBudget::production().max_total_events,
        usize::MAX,
        "production() must not delete valid durable rows by default (#1480)",
    );

    // Production keeps the same scan bounds as default().
    let prod = GcBudget::production();
    let def = GcBudget::default();
    assert_eq!(prod.max_events_per_step, def.max_events_per_step);
    assert_eq!(prod.max_duration_ms, def.max_duration_ms);
    assert_eq!(prod.max_total_events, def.max_total_events);

    let explicit = GcBudget::with_durable_event_ceiling(DEFAULT_DURABLE_EVENT_CEILING);
    assert_eq!(explicit.max_total_events, DEFAULT_DURABLE_EVENT_CEILING);
}

/// The production budget itself must not LRU-delete valid rows, even with an
/// empty pin set. Correctness deletes (expiry/tombstones) still run elsewhere;
/// this test only guards against reintroducing the implicit durable row cap.
#[test]
fn production_budget_does_not_lru_delete_valid_rows() {
    let (store, _dir) = open_tmp();

    let mut ids = Vec::new();
    for i in 0..60usize {
        let raw = signed_event(1, 1_700_000_000 + i as u64, &format!("event-{i}"), None);
        let id = raw.id_bytes().expect("fixture event id");
        let outcome = store
            .insert(verified(raw), &"wss://r/".into(), 1_000_000)
            .unwrap_or_else(|e| panic!("insert #{i} failed: {e}"));
        assert!(matches!(outcome, InsertOutcome::Inserted { .. }));
        ids.push(id);
    }

    let report = store
        .gc_step_with_pins(GcBudget::production(), 1_700_100_000, &HashSet::new())
        .expect("production gc_step_with_pins");

    assert_eq!(
        report.lru_evicted, 0,
        "production budget must not LRU-delete valid durable rows"
    );
    for id in ids {
        assert!(
            store.get_by_id(&id).expect("get_by_id").is_some(),
            "valid row must remain queryable after production GC"
        );
    }
}

/// Explicit finite durable retention still evicts least-recently-accessed rows
/// down to the requested ceiling. This keeps the old guarded deletion machinery
/// covered without making it the production default.
#[test]
fn explicit_durable_ceiling_evicts_to_ceiling_with_empty_pins() {
    let (store, _dir) = open_tmp();

    const N: usize = 60;
    const CEILING: usize = 40;
    insert_n(&store, N, 1_700_000_000);

    let budget = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 60_000,
        ..GcBudget::with_durable_event_ceiling(CEILING)
    };
    let now_secs = 1_700_100_000u64;

    // Empty pin set: nothing protected. The explicit finite durable-retention
    // ceiling trims the overage.
    let pins: HashSet<EventId> = HashSet::new();
    let report = store
        .gc_step_with_pins(budget, now_secs, &pins)
        .expect("gc_step_with_pins");

    assert!(
        report.lru_evicted >= N - CEILING,
        "expected ≥{} LRU evictions ({N} - ceiling={CEILING}), got {}",
        N - CEILING,
        report.lru_evicted,
    );
}
