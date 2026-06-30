//! Stage-1 TDD tests for GitHub #1090: derived pin set for gc_step.
//!
//! RED phase: These tests verify the new gc_step(budget, now_secs, pins)
//! behaviour BEFORE the implementation is complete. They drive the design.
//!
//! (a) pinned events survive a gc pass that would evict them
//! (b) claims sub-dbs are gone (compile-time, no test needed — deletion IS the test)
//! (c) existing gc_step tests pass with the new signature (no change to existing
//!     tests needed — they'll call the new signature via store.gc_step which
//!     passes an empty HashSet internally)

#![cfg(feature = "lmdb-backend")]

use std::collections::HashSet;

use crate::types::{EventId, GcBudget, InsertOutcome};
use crate::EventStore;

use super::test_fixtures::{open_tmp, signed_event, verified};

/// (a) A pinned event must survive a gc pass that would otherwise evict it
/// via Phase-2 LRU eviction.
///
/// Setup: insert 5 events into a store with ceiling=3, pin 2 of them via the
/// new `gc_step_with_pins` call path. The LRU eviction must remove only
/// non-pinned events.
#[test]
fn gc_step_never_evicts_pinned_events() {
    let (store, _dir) = open_tmp();

    // Insert 5 events.
    let base_ts = 1_700_000_000u64;
    let mut ids: Vec<EventId> = Vec::new();
    for i in 0..5usize {
        let raw = signed_event(1, base_ts + i as u64, &format!("event-{i}"), None);
        let id = raw.id_bytes().expect("id");
        let outcome = store
            .insert(verified(raw), &"wss://r/".into(), 1_000_000 + i as u64)
            .expect("insert");
        if matches!(outcome, InsertOutcome::Inserted { .. }) {
            ids.push(id);
        }
    }
    assert_eq!(ids.len(), 5, "all 5 events must insert");

    // Pin the first 2 events via the new derived-pin path.
    let mut pins: HashSet<EventId> = HashSet::new();
    pins.insert(ids[0]);
    pins.insert(ids[1]);

    // GC with ceiling=3 must evict 2 events, but ONLY from the non-pinned 3.
    let budget = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 60_000,
        max_total_events: 3, // 5 events → 2 must be evicted
    };
    let now_secs = base_ts + 1_000;

    let report = store
        .gc_step_with_pins(budget, now_secs, &pins)
        .expect("gc_step");
    assert_eq!(
        report.lru_evicted, 2,
        "must evict exactly 2 non-pinned events, got {}",
        report.lru_evicted,
    );

    // The 2 pinned events must still be present.
    for &pinned_id in &[ids[0], ids[1]] {
        assert!(
            store.get_by_id(&pinned_id).expect("get_by_id").is_some(),
            "pinned event must not be evicted"
        );
    }
}

/// gc_step on the public trait passes an empty pin set — no eviction of anything
/// when ceiling is large, pinned-set is empty.
#[test]
fn gc_step_trait_passthrough_works_with_new_signature() {
    let (store, _dir) = open_tmp();
    let base_ts = 1_700_000_000u64;
    for i in 0..3usize {
        let raw = signed_event(1, base_ts + i as u64, &format!("passthrough-{i}"), None);
        store
            .insert(verified(raw), &"wss://r/".into(), 1_000_000)
            .expect("insert");
    }
    let budget = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 60_000,
        max_total_events: usize::MAX,
    };
    // Public trait method (no explicit pins) must not error.
    let report = store.gc_step(budget, base_ts + 1_000).expect("gc_step");
    assert_eq!(
        report.lru_evicted, 0,
        "no evictions when ceiling=usize::MAX"
    );
}
