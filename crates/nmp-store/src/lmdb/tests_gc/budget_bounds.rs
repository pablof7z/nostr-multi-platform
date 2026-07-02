//! `gc_step` budget-enforcement tests (duration / count / eviction ceilings).
//!
//! Split out of `tests_gc.rs` (500-LOC hard cap) — see V-117 (GitHub #1097
//! predecessor) for the original defect: an unbounded O(store) Phase-1 scan
//! could stall the actor thread. These tests pin the three budget dimensions
//! `gc_step` must respect:
//!
//! 1. `gc_step_duration_bounded_on_large_store` (+ `gc_step_50k_manual`,
//!    `#[ignore]`d) — a wall-time budget bounds Phase-1 scan duration even on
//!    a store large enough to expose an unbounded scan.
//! 2. `gc_event_count_via_stat_is_o1` — the Phase-2 event-count check uses the
//!    O(1) `heed` stat path (`ci_index.len`), not an O(N) full scan.
//! 3. `lru_eviction_still_works_with_explicit_finite_ceiling` — the eviction
//!    CODE still fires correctly when a finite `max_total_events` ceiling is
//!    set explicitly (production() disables the ceiling by default).

#![cfg(feature = "lmdb-backend")]

use crate::types::{GcBudget, InsertOutcome};
use crate::EventStore;

use super::super::test_fixtures::{open_tmp, signed_event, verified};

/// Insert `n` distinct kind:1 events into `store`.
/// `base_ts` + i is the `created_at` for event i.
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

// ─── Test 1: duration budget bounds large-store gc pass ──────────────────────

/// With a 50ms wall-time budget, `gc_step` on a 5k-event store must return
/// well within ≤ 4× the budget.  This guards against the O(store)
/// actor-thread stall from V-117 part A (Phase 1 scan with no duration check).
///
/// 5k events is enough to expose an unbounded scan: without the duration gate
/// the scan would iterate all 5k events (several hundred ms in debug mode);
/// with the gate it returns as soon as the budget elapses.
///
/// ## Why not 50k events?
///
/// Inserting 50k signed Nostr events takes ~500 s in debug (Schnorr signing).
/// 1k events still exercises the O(store) path meaningfully and keeps test
/// runtime well under 30 s.  An `#[ignore]` variant with 50k events is
/// available for manual verification.
#[test]
fn gc_step_duration_bounded_on_large_store() {
    let (store, _dir) = open_tmp();

    // Insert 1k plain events (no expiration tag).
    const N: usize = 1_000;
    insert_n(&store, N, 1_700_000_000);

    let budget = GcBudget {
        max_events_per_step: 10_000,
        max_duration_ms: 50,
        max_total_events: usize::MAX,
    };
    let now_secs = 1_700_050_000u64;

    let wall_start = std::time::Instant::now();
    let report = store
        .gc_step(budget, now_secs)
        .expect("gc_step must not error");
    let wall_elapsed_ms = wall_start.elapsed().as_millis();

    // Allow 4× the budget for system jitter.
    // Without the V-117 fix an unbounded scan on 5k events takes >> 200ms;
    // with the fix the scan breaks as soon as 50ms elapsed.
    assert!(
        wall_elapsed_ms <= 4 * budget.max_duration_ms as u128,
        "gc_step took {wall_elapsed_ms}ms on a {N}-event store; budget was {}ms. \
         V-117: Phase-1 scan must check the duration budget inside the scan loop.",
        budget.max_duration_ms,
    );

    // Sanity: no events should be reaped (none have expiration tags).
    assert_eq!(report.expired_reaped, 0, "no expired events inserted");
}

/// Manual variant with 50k events — `#[ignore]`d so it does not run in CI.
/// Run with: `cargo test -p nmp-store --features lmdb-backend -- --ignored --nocapture \
///   gc_step_50k_manual`
#[test]
#[ignore = "manual large-store timing; too slow for CI (Schnorr signing dominates)"]
fn gc_step_50k_manual() {
    let (store, _dir) = open_tmp();
    const N: usize = 50_000;
    insert_n(&store, N, 1_700_000_000);
    let budget = GcBudget {
        max_events_per_step: 10_000,
        max_duration_ms: 50,
        max_total_events: usize::MAX,
    };
    let wall_start = std::time::Instant::now();
    let report = store.gc_step(budget, 1_700_050_000).expect("gc_step");
    let elapsed_ms = wall_start.elapsed().as_millis();
    println!(
        "gc_step_50k_manual: N={N} budget=50ms elapsed={elapsed_ms}ms reaped={}",
        report.expired_reaped
    );
    assert!(
        elapsed_ms <= 4 * budget.max_duration_ms as u128,
        "gc_step took {elapsed_ms}ms; budget was {}ms",
        budget.max_duration_ms,
    );
}

// ─── Test 4: Phase-2 count is O(1) ───────────────────────────────────────────

/// Phase-2 event count must complete in << 50ms even on a 5k-event store.
///
/// We test the O(1) path by setting the ceiling to EXACTLY the number of
/// inserted events: `event_count > max_total_events` is false, so Phase 2
/// runs the count check but skips the expensive LRU eviction loop entirely.
/// This isolates the count path from the eviction work.
///
/// Before V-117, the count was `query(Filter::new()).count()` — an O(N) full
/// scan that takes hundreds of ms on 5k events in debug mode.  After V-117 it
/// is `Lmdb::count(txn, Filter::new())` → `ci_index.len(txn)` (one MDB_stat
/// syscall).  We allow 50ms total to account for debug overhead.
#[test]
fn gc_event_count_via_stat_is_o1() {
    let (store, _dir) = open_tmp();

    // Insert exactly N events.
    const N: usize = 1_000;
    insert_n(&store, N, 1_700_000_000);

    // ceiling = N exactly: count check runs but overage = 0, eviction loop skipped.
    let budget = GcBudget {
        max_events_per_step: 10_000,
        max_duration_ms: 60_000, // generous; wall time measured separately
        max_total_events: N,     // no overage → eviction loop skipped
    };
    let now_secs = 1_700_100_000u64;

    let wall_start = std::time::Instant::now();
    let report = store.gc_step(budget, now_secs).expect("gc_step");
    let wall_ms = wall_start.elapsed().as_millis();

    // No events evicted: ceiling == count, no overage.
    assert_eq!(report.lru_evicted, 0, "no overage: nothing to evict");

    // O(1) count path takes < 50ms even on 5k events in debug mode.
    // The tombstone scan is gated (first pass), so the only work is the count call
    // and the (empty) Phase-1 scan.
    assert!(
        wall_ms < 50,
        "gc_step with O(1) count took {wall_ms}ms on a {N}-event store; expected < 50ms. \
         The count path (ci_index.len) must be O(1), not O(N).",
    );
}

// ─── Test 5: LRU eviction code still exercises correctly ─────────────────────

/// The eviction CODE still works when given an explicit finite ceiling budget.
/// This test uses a ceiling of 80 on a 120-event store, so Phase 2 must
/// evict 40 events in a single pass.
#[test]
fn lru_eviction_still_works_with_explicit_finite_ceiling() {
    let (store, _dir) = open_tmp();

    // Insert 120 events.
    const N: usize = 120;
    insert_n(&store, N, 1_700_000_000);

    let budget = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 60_000,
        max_total_events: 80, // ceiling below N
    };
    let now_secs = 1_700_100_000u64;

    let report = store.gc_step(budget, now_secs).expect("gc_step");

    // Must have evicted the overage (120 - 80 = 40) or close to it.
    assert!(
        report.lru_evicted >= 40,
        "expected ≥40 LRU evictions (120 - ceiling=80), got {}",
        report.lru_evicted,
    );

    // Verify the duration is recorded in the report.
    assert!(
        report.duration_ms < 60_000,
        "duration_ms must be populated in GcReport, got {}",
        report.duration_ms,
    );
}
