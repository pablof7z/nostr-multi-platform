//! #1090 Stage 3 — production GC budget re-enables the finite LRU ceiling.
//!
//! Stage 1 (#1090) wired the kernel-derived pin set into
//! [`EventStore::gc_step_with_pins`] but kept `GcBudget::production()` at
//! `max_total_events = usize::MAX` (eviction disabled) pending the Stage-2
//! watermark-coherence decision. Stage 2 added floor-coherent pinning
//! (`Kernel::derive_store_pin_set` pins every stored event at or below each
//! active floored shape's `since`-floor), so a finite ceiling can no longer
//! punch a hole the floored self-healing REQ would never re-request. Stage 3
//! flips `production()` to `max_total_events = HOT_EVENT_CEILING`.
//!
//! These tests live in their own file (not appended to the already-baselined
//! `tests_gc.rs`, which is at the 500-LOC hard cap) per the AGENTS.md file-size
//! rule.

#![cfg(feature = "lmdb-backend")]

use std::collections::HashSet;

use crate::types::{EventId, GcBudget, InsertOutcome, HOT_EVENT_CEILING};
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

/// #1090 Stage 3 — `GcBudget::production()` enables the finite LRU ceiling
/// (`max_total_events == HOT_EVENT_CEILING`), reversing the Stage-1 temporary
/// `usize::MAX` disable. The on-device 60s GC pass relies on this contract;
/// floor-coherence (Stage 2, `Kernel::derive_store_pin_set`) is what makes the
/// finite ceiling safe.
#[test]
fn gc_production_budget_ceiling_is_hot_event_ceiling() {
    assert_eq!(
        GcBudget::production().max_total_events,
        HOT_EVENT_CEILING,
        "production() must enable the finite ceiling (#1090 Stage 3)",
    );

    // Scan bounds are unchanged from default() — only the ceiling differs.
    let prod = GcBudget::production();
    let def = GcBudget::default();
    assert_eq!(prod.max_events_per_step, def.max_events_per_step);
    assert_eq!(prod.max_duration_ms, def.max_duration_ms);
    assert_eq!(
        def.max_total_events,
        usize::MAX,
        "default() keeps eviction disabled (tests opt into a finite ceiling)",
    );
}

/// #1090 Stage 3 — an over-ceiling store with NO active floored shapes (empty
/// pin set) evicts least-recently-accessed events down to the ceiling via
/// `gc_step_with_pins`. This mirrors the production call shape
/// (`gc_step_with_pins(production(), now, &pins)`) when the kernel holds no
/// live working set; we use a small synthetic ceiling so the test need not
/// Schnorr-sign `HOT_EVENT_CEILING` events.
#[test]
fn gc_over_ceiling_no_floor_evicts_to_ceiling_with_empty_pins() {
    let (store, _dir) = open_tmp();

    const N: usize = 60;
    const CEILING: usize = 40;
    insert_n(&store, N, 1_700_000_000);

    let budget = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 60_000,
        max_total_events: CEILING,
    };
    let now_secs = 1_700_100_000u64;

    // Empty pin set: nothing protected. With the production ceiling enabled and
    // no floored shape active, Phase-2 LRU eviction trims the overage.
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
