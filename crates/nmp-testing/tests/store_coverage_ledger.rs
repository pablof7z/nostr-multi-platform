//! K3 Stage D1 — coverage-ledger store-layer tests (ADR-0072 §3).
//!
//! Exercises the `EventStore::record_coverage` / `get_coverage` write path on
//! BOTH backends via `for_each_backend!`. These are pure store-layer tests: the
//! kernel flag gate and the EOSE/NEG-DONE wiring are covered separately in
//! `nmp-core`. Here we prove the ledger primitive itself is sound — keyed by
//! `(filter_hash, relay)`, monotonic, and isolated per key.

use nmp_testing::for_each_backend;
use nmp_testing::store_harness::StoreHarness;

const FH: &str = "deadbeefdeadbeef";
const RELAY: &str = "wss://relay.example";

for_each_backend!(coverage_absent_reads_none, |h: &mut StoreHarness| {
    assert_eq!(h.store.get_coverage(FH, RELAY), None);
});

for_each_backend!(coverage_record_then_read, |h: &mut StoreHarness| {
    h.store.record_coverage(FH, RELAY, 5_000);
    assert_eq!(h.store.get_coverage(FH, RELAY), Some(5_000));
});

for_each_backend!(coverage_advances_monotonically, |h: &mut StoreHarness| {
    h.store.record_coverage(FH, RELAY, 5_000);
    // A later completion raises the proven bound.
    h.store.record_coverage(FH, RELAY, 9_000);
    assert_eq!(h.store.get_coverage(FH, RELAY), Some(9_000));
    // An older completion can NEVER lower it (downward-closed monotonicity).
    h.store.record_coverage(FH, RELAY, 1_000);
    assert_eq!(h.store.get_coverage(FH, RELAY), Some(9_000));
});

for_each_backend!(coverage_is_keyed_by_filter_hash, |h: &mut StoreHarness| {
    h.store.record_coverage(FH, RELAY, 5_000);
    // A different filter_hash on the same relay is a distinct row.
    assert_eq!(h.store.get_coverage("0000000000000000", RELAY), None);
});

for_each_backend!(coverage_is_keyed_by_relay, |h: &mut StoreHarness| {
    h.store.record_coverage(FH, RELAY, 5_000);
    // The same filter_hash on a different relay is a distinct row — coverage on
    // one relay says nothing about coverage on another.
    assert_eq!(h.store.get_coverage(FH, "wss://other.example"), None);
});

for_each_backend!(coverage_zero_is_noop, |h: &mut StoreHarness| {
    // Recording "no coverage" must not materialise a misleading row.
    h.store.record_coverage(FH, RELAY, 0);
    assert_eq!(h.store.get_coverage(FH, RELAY), None);
});
