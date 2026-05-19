//! §2.8 NIP-40 expiration scheduling + GC reaping tests.
//!
//! See `docs/design/lmdb/tests.md` §2.8.

use nmp_core::store::{GcBudget, InsertOutcome, RejectReason, TombstoneOrigin};
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX};

// Use a fixed "now" to make tests deterministic.
const NOW_SECS: u64 = 1_700_000_000;
const NOW_MS: u64 = NOW_SECS * 1_000;

for_each_backend!(expiring_event_appears_in_scan_before_gc, |h: &mut StoreHarness| {
    // Insert a kind:1 expiring in 1 second from "now".
    let exp = NOW_SECS + 1;
    let ev = h.make_event_with_tags(ALICE_HEX, 1, NOW_SECS, vec![
        vec!["expiration".to_string(), exp.to_string()],
    ]);
    let id = ev.id_bytes();
    h.insert_raw(ev, "wss://t/", NOW_MS);

    // Before expiry window: scan_expiring_before(now + 5) should return it.
    let results: Vec<_> = h.store
        .scan_expiring_before(NOW_SECS + 5, 10)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].raw.id_bytes(), id);
});

for_each_backend!(gc_step_reaps_expired_events, |h: &mut StoreHarness| {
    // Insert an event that has already expired (expiration = NOW_SECS - 1).
    // NIP-40: an event with expiration already in the past on ARRIVAL is Rejected.
    // For the GC test, we need an event that was valid on arrival but expires later.
    // Simulate: insert with received_at_ms BEFORE expiry.
    let exp = NOW_SECS + 2;
    let ev = h.make_event_with_tags(ALICE_HEX, 1, NOW_SECS, vec![
        vec!["expiration".to_string(), exp.to_string()],
    ]);
    // received_at is before expiry — event is valid on arrival.
    // (This event won't be reaped in this test — its expiry is far in the future.)
    h.insert_raw(ev, "wss://t/", (NOW_SECS - 10) * 1000);

    // GC at NOW_SECS + 2 (past expiry) — should reap the event.
    // We have to fake "now" inside gc_step. The MemEventStore uses real system time,
    // so we insert an event whose expiration is already past current real time.
    // In unit tests, we use a dedicated GC path with a known-past expiration.
    // For now, verify the tombstone is created when gc_step runs with the event expired.
    //
    // Alternate approach: use an expiration that is guaranteed to be in the past
    // regardless of when the test runs (unix epoch 1 second).
    let ev_past = h.make_event_with_tags(ALICE_HEX, 1, 1_000, vec![
        vec!["expiration".to_string(), "2".to_string()], // expires at unix second 2
    ]);
    let past_id = ev_past.id_bytes();
    // Insert with received_at before expiry (unix ms 1 = received before exp=2).
    h.insert_raw(ev_past, "wss://t/", 1);
    h.assert_present(&past_id);

    let report = h.store.gc_step(GcBudget { max_events_per_step: 100, max_duration_ms: 1000 }).unwrap();
    assert!(report.expired_reaped >= 1, "gc_step should reap at least the expired event");

    // Verify tombstone with NIP40Expiry origin.
    let tombs = h.store.tombstones_for(&past_id).unwrap();
    assert!(!tombs.is_empty(), "tombstone should exist after GC reap");
    assert_eq!(tombs[0].origin, TombstoneOrigin::NIP40Expiry);
});

for_each_backend!(expired_on_arrival_is_rejected, |h: &mut StoreHarness| {
    // An event with expiration already in the past at received_at_ms time.
    let ev = h.make_event_with_tags(ALICE_HEX, 1, 1_000, vec![
        vec!["expiration".to_string(), "999".to_string()], // exp < created_at even
    ]);
    let id = ev.id_bytes();
    // received_at_ms converts to NOW_SECS which is > 999.
    let o = h.insert_raw(ev, "wss://t/", NOW_MS);
    assert!(
        matches!(o, InsertOutcome::Rejected { reason: RejectReason::ExpiredOnArrival, .. }),
        "expected ExpiredOnArrival, got {o:?}"
    );
    h.assert_absent(&id);
});

for_each_backend!(tombstoned_expired_blocks_reinsert, |h: &mut StoreHarness| {
    // Insert an event that will expire, let GC reap it, then try to reinsert.
    let ev = h.make_event_with_tags(ALICE_HEX, 1, 1_000, vec![
        vec!["expiration".to_string(), "2".to_string()],
    ]);
    let ev_clone = ev.clone();
    h.insert_raw(ev, "wss://t/", 1);

    // Reap.
    h.store.gc_step(GcBudget { max_events_per_step: 100, max_duration_ms: 1000 }).unwrap();

    // Reinsert (received_at in the past too, so not ExpiredOnArrival but Tombstoned).
    let o = h.insert_raw(ev_clone, "wss://t/", 1);
    assert!(
        matches!(
            o,
            InsertOutcome::Tombstoned { origin: TombstoneOrigin::NIP40Expiry, .. }
            | InsertOutcome::Rejected { .. }
        ),
        "reinsert after GC should be Tombstoned or Rejected, got {o:?}"
    );
});
