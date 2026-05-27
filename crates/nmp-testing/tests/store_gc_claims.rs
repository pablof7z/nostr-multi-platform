//! §2.10 Claims + GC tests.
//!
//! See `docs/design/lmdb/tests.md` §2.10.

use nmp_core::store::{ClaimerId, GcBudget, StoreError};
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX};

for_each_backend!(claim_pins_events, |h: &mut StoreHarness| {
    let ev = h.make_event(ALICE_HEX, 1, 1_000);
    let id = ev.id_bytes();
    h.insert_raw(ev, "wss://t/", 1_000_000);

    let claimer = ClaimerId(1);
    h.store.register_view_cover(claimer, 100).unwrap();
    h.store.claim(claimer, &[id]).unwrap();

    h.assert_present(&id);
});

for_each_backend!(release_removes_pins, |h: &mut StoreHarness| {
    let ev = h.make_event(ALICE_HEX, 1, 1_000);
    let id = ev.id_bytes();
    h.insert_raw(ev, "wss://t/", 1_000_000);

    let claimer = ClaimerId(1);
    h.store.register_view_cover(claimer, 100).unwrap();
    h.store.claim(claimer, &[id]).unwrap();
    h.store.release(claimer).unwrap();

    // After release, the event is still in primary (release doesn't delete).
    h.assert_present(&id);
});

for_each_backend!(claim_over_budget_returns_error, |h: &mut StoreHarness| {
    let claimer = ClaimerId(2);
    h.store.register_view_cover(claimer, 2).unwrap();

    // Insert 3 events and try to claim all of them (exceeds budget of 2).
    let ids: Vec<_> = (0..3)
        .map(|i| {
            let ev = h.make_event(ALICE_HEX, 1, i as u64 + 1_000);
            let id = ev.id_bytes();
            h.insert_raw(ev, "wss://t/", (i as u64 + 1) * 1_000_000);
            id
        })
        .collect();

    let err = h.store.claim(claimer, &ids);
    assert!(
        matches!(err, Err(StoreError::OverPinned { .. })),
        "claiming over budget should return OverPinned, got {err:?}"
    );
});

for_each_backend!(hot_set_hint_does_not_error, |h: &mut StoreHarness| {
    let ev = h.make_event(ALICE_HEX, 1, 1_000);
    let id = ev.id_bytes();
    h.insert_raw(ev, "wss://t/", 1_000_000);

    // hot_set_hint is best-effort; the mem backend is a no-op.
    h.store.hot_set_hint(&[id]).unwrap();
    h.assert_present(&id);
});

for_each_backend!(gc_step_runs_without_error, |h: &mut StoreHarness| {
    // Insert some events, run GC — should complete without errors.
    for i in 0..10u64 {
        let ev = h.make_event(ALICE_HEX, 1, i + 1_000);
        h.insert_raw(ev, "wss://t/", (i + 1) * 1_000_000);
    }

    let report = h
        .store
        .gc_step(GcBudget {
            max_events_per_step: 50,
            max_duration_ms: 500,
        })
        .unwrap();

    // No events should be reaped (none are expired).
    assert_eq!(report.expired_reaped, 0);
});
