//! K3 Stage D3 — eviction⇄ledger coherence BACKSTOP, store layer (ADR-0056 §3.D3).
//!
//! The backstop leg: even if the floor-coherent pin set is bypassed or
//! budget-truncated, LRU eviction MUST NOT leave the coverage ledger
//! over-claiming a range it no longer holds. If `gc_step` deletes an event
//! whose `created_at <= covered_through` for an active covered shape, it MUST
//! lower that row's `covered_through` to just below the oldest SURVIVING covered
//! event (or clear it) **in the same transaction as the delete** — so a
//! subsequent recompile re-fetches the gap rather than reading a poisoned floor.
//!
//! These are pure store-layer tests on BOTH backends via `for_each_backend!`
//! (the ledger has Mem + LMDB impls and the eviction+ledger path is
//! backend-sensitive — Mem's single lock vs LMDB's single write txn). The
//! shape→event matcher is supplied by the caller (the kernel owns the predicate,
//! D0), so here the guard's `matches` closure is a hand-built stand-in keyed on
//! author+kind, exactly the shape a follow-feed `AuthorKind` interest produces.
//!
//! Sabotage RED: with the backstop removed (no `covered_through` lowering on a
//! below-floor eviction) the ledger keeps the high bound and these asserts fail
//! — the exact permanent-hole class the memory review flagged.

#![cfg(feature = "lmdb-backend")]

use nmp_store::{CoverageGuard, GcBudget};
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX, BOB_HEX};

const RELAY: &str = "wss://relay.example/";
const FH: &str = "a11ce0000000a11c";

/// A guard whose `matches` predicate mirrors a follow-feed `AuthorKind` shape:
/// author == Alice AND kind == 1. `covered_through` is the ledger bound the
/// eviction must keep honest.
fn alice_kind1_guard(covered_through: u64) -> CoverageGuard {
    CoverageGuard {
        filter_hash: FH.to_string(),
        relay: RELAY.to_string(),
        covered_through,
        matches: std::sync::Arc::new(|_id: &str, author: &str, kind: u32, _ts: u64, _tags: &[Vec<String>]| {
            author == ALICE_HEX && kind == 1
        }),
    }
}

/// A finite-ceiling budget that forces LRU eviction of all-but-`ceiling`
/// non-pinned events in one pass.
fn evicting_budget(ceiling: usize) -> GcBudget {
    GcBudget {
        max_events_per_step: 10_000,
        max_duration_ms: 10_000,
        max_total_events: ceiling,
    }
}

// ── Leg 2 (backstop) — eviction below covered_through lowers the row ──────────

for_each_backend!(
    eviction_below_covered_through_lowers_the_row,
    |h: &mut StoreHarness| {
        // Alice authored kind:1 at t=100, 200, 300; the ledger claims coverage
        // through 300 (a completed sync). No event is pinned.
        let e100 = h.make_event_with_id(&format!("{:0>64x}", 100u64), ALICE_HEX, 1, 100);
        let e200 = h.make_event_with_id(&format!("{:0>64x}", 200u64), ALICE_HEX, 1, 200);
        let e300 = h.make_event_with_id(&format!("{:0>64x}", 300u64), ALICE_HEX, 1, 300);
        let id100 = e100.id_bytes().unwrap();
        let id300 = e300.id_bytes().unwrap();
        h.insert_raw(e100, RELAY, 100_000);
        h.insert_raw(e200, RELAY, 200_000);
        h.insert_raw(e300, RELAY, 300_000);
        h.store.record_coverage(FH, RELAY, 300);
        assert_eq!(h.store.get_coverage(FH, RELAY), Some(300));

        // Touch e300 so it is the most-recently-accessed → LRU evicts the two
        // OLDEST (e100, e200), both below covered_through=300.
        let _ = h.store.get_by_id(&id300);
        let _ = id300;

        let guards = vec![alice_kind1_guard(300)];
        let report = h
            .store
            .gc_step_with_pins_and_coverage(
                evicting_budget(1),
                1_000,
                &std::collections::HashSet::new(),
                &guards,
            )
            .expect("gc_step_with_pins_and_coverage");
        assert!(report.lru_evicted >= 2, "expected ≥2 LRU evictions, got {}", report.lru_evicted);

        // e100 was evicted; it is below covered_through=300. The oldest SURVIVING
        // covered event is e300 (t=300), so the ledger must be lowered to JUST
        // BELOW the oldest evicted-below-floor event's bound — i.e. it can no
        // longer claim [0, 300]. The honest new bound is < the oldest evicted
        // covered event (100), so the row is lowered to 99 (or cleared).
        let lowered = h
            .store
            .get_coverage(FH, RELAY)
            .expect("row must still exist or be cleared, not over-claim");
        assert!(
            lowered < 100,
            "covered_through must drop below the oldest evicted covered event \
             (100); got {lowered} — the ledger still over-claims a hole"
        );

        // The evicted below-floor event is gone (so a re-fetch is required).
        h.assert_absent(&id100);
    }
);

for_each_backend!(
    eviction_above_covered_through_does_not_lower_the_row,
    |h: &mut StoreHarness| {
        // Coverage only through 150. Events at 100 (below) and 200/300 (above).
        let e100 = h.make_event_with_id(&format!("{:0>64x}", 100u64), ALICE_HEX, 1, 100);
        let e200 = h.make_event_with_id(&format!("{:0>64x}", 200u64), ALICE_HEX, 1, 200);
        let e300 = h.make_event_with_id(&format!("{:0>64x}", 300u64), ALICE_HEX, 1, 300);
        let id100 = e100.id_bytes().unwrap();
        let id200 = e200.id_bytes().unwrap();
        let id300 = e300.id_bytes().unwrap();
        h.insert_raw(e100, RELAY, 100_000);
        h.insert_raw(e200, RELAY, 200_000);
        h.insert_raw(e300, RELAY, 300_000);
        h.store.record_coverage(FH, RELAY, 150);

        // Pin e100 (below floor) so only e200/e300 (ABOVE floor) are evictable;
        // touch e300 so e200 is the LRU victim.
        let mut pins = std::collections::HashSet::new();
        pins.insert(id100);
        let _ = h.store.get_by_id(&id300);

        let guards = vec![alice_kind1_guard(150)];
        h.store
            .gc_step_with_pins_and_coverage(evicting_budget(2), 1_000, &pins, &guards)
            .expect("gc");

        // e200 (created_at=200 > covered_through=150) was evicted, but it is
        // ABOVE the covered bound — evicting it cannot create a hole in [0, 150],
        // so the row must be UNTOUCHED.
        assert_eq!(
            h.store.get_coverage(FH, RELAY),
            Some(150),
            "evicting an event above covered_through must NOT lower the row"
        );
        h.assert_absent(&id200);
    }
);

for_each_backend!(
    eviction_non_matching_event_does_not_lower_the_row,
    |h: &mut StoreHarness| {
        // The guard matches Alice/kind1. Evict a BOB event below the bound: it
        // does not match the covered shape, so the row stays.
        let bob = h.make_event_with_id(&format!("{:0>64x}", 50u64), BOB_HEX, 1, 50);
        let alice = h.make_event_with_id(&format!("{:0>64x}", 300u64), ALICE_HEX, 1, 300);
        let bob_id = bob.id_bytes().unwrap();
        let alice_id = alice.id_bytes().unwrap();
        h.insert_raw(bob, RELAY, 50_000);
        h.insert_raw(alice, RELAY, 300_000);
        h.store.record_coverage(FH, RELAY, 300);

        // Touch Alice so Bob is the LRU victim.
        let _ = h.store.get_by_id(&alice_id);

        let guards = vec![alice_kind1_guard(300)];
        h.store
            .gc_step_with_pins_and_coverage(evicting_budget(1), 1_000, &std::collections::HashSet::new(), &guards)
            .expect("gc");

        h.assert_absent(&bob_id);
        assert_eq!(
            h.store.get_coverage(FH, RELAY),
            Some(300),
            "evicting an event the covered shape does NOT match must not lower it"
        );
    }
);

for_each_backend!(
    no_guards_is_byte_identical_to_plain_gc_step_with_pins,
    |h: &mut StoreHarness| {
        // Flag-off regression: with NO coverage guards, the new path must behave
        // exactly like gc_step_with_pins — coverage rows are never touched.
        let e100 = h.make_event_with_id(&format!("{:0>64x}", 100u64), ALICE_HEX, 1, 100);
        let e300 = h.make_event_with_id(&format!("{:0>64x}", 300u64), ALICE_HEX, 1, 300);
        let id100 = e100.id_bytes().unwrap();
        let id300 = e300.id_bytes().unwrap();
        h.insert_raw(e100, RELAY, 100_000);
        h.insert_raw(e300, RELAY, 300_000);
        h.store.record_coverage(FH, RELAY, 300);
        let _ = h.store.get_by_id(&id300);

        h.store
            .gc_step_with_pins_and_coverage(
                evicting_budget(1),
                1_000,
                &std::collections::HashSet::new(),
                &[], // no guards
            )
            .expect("gc");

        h.assert_absent(&id100);
        // No guards ⇒ the row is left exactly as recorded (no lowering).
        assert_eq!(h.store.get_coverage(FH, RELAY), Some(300));
    }
);

// ── Leg 1 support — max coverage across relays for a filter_hash ──────────────

for_each_backend!(
    coverage_max_for_filter_hash_takes_the_highest_across_relays,
    |h: &mut StoreHarness| {
        h.store.record_coverage(FH, "wss://r1/", 100);
        h.store.record_coverage(FH, "wss://r2/", 500);
        h.store.record_coverage(FH, "wss://r3/", 300);
        // A different filter_hash must not bleed in.
        h.store.record_coverage("ffffffffffffffff", "wss://r1/", 9_000);

        assert_eq!(
            h.store.coverage_max_for_filter_hash(FH),
            Some(500),
            "max covered_through across this filter_hash's relays"
        );
        assert_eq!(
            h.store.coverage_max_for_filter_hash("0000000000000000"),
            None,
            "an unknown filter_hash has no coverage"
        );
    }
);
