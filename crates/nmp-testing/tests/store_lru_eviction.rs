//! V-60 — LRU eviction tests for `gc_step`.
//!
//! Verifies:
//! 1. `lru_evicted > 0` after seeding past the ceiling.
//! 2. Store is at or under the ceiling after `gc_step`.
//! 3. Least-recently-READ events are evicted; recently-read events survive.
//! 4. Pinned (claimed) events are never evicted.
//! 5. Secondary indexes (`list_events_seen_on`) are consistent after eviction.
//! 6. `now_secs` is used — not wall-clock — so passing a fixed ts yields
//!    deterministic behaviour (clock test / D7 verification).
//! 7. Parity: mem and lmdb evict the same events given the same access order.

use nmp_core::store::{ClaimerId, GcBudget};
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX};

// A fixed "now" that is far in the future relative to any fixture event.
// No events will expire at this time; only the LRU ceiling triggers eviction.
const NOW_SECS: u64 = 1_700_000_000;

// ─── Test 1: basic eviction ───────────────────────────────────────────────────

for_each_backend!(lru_evicts_when_over_ceiling, |h: &mut StoreHarness| {
    // Insert 5 events.
    let ids: Vec<_> = (0..5u64)
        .map(|i| {
            let ev = h.make_event(ALICE_HEX, 1, 1_000 + i);
            let id = ev.id_bytes().expect("fixture: valid hex");
            h.insert_raw(ev, "wss://t/", (1_000 + i) * 1_000);
            id
        })
        .collect();
    assert_eq!(ids.len(), 5);

    // Ceiling = 3 → must evict 2.
    let report = h
        .store
        .gc_step(
            GcBudget {
                max_events_per_step: 100,
                max_duration_ms: 5_000,
                max_total_events: 3,
            },
            NOW_SECS,
        )
        .unwrap();

    assert!(
        report.lru_evicted >= 2,
        "expected at least 2 lru_evicted, got {}",
        report.lru_evicted
    );
    assert_eq!(report.expired_reaped, 0, "no events should expire");
});

// ─── Test 2: store is at or under ceiling after gc_step ───────────────────────

for_each_backend!(lru_store_under_ceiling_after_gc, |h: &mut StoreHarness| {
    // Insert 8 events.
    for i in 0..8u64 {
        let ev = h.make_event(ALICE_HEX, 1, 1_000 + i);
        h.insert_raw(ev, "wss://t/", (1_000 + i) * 1_000);
    }

    let ceiling = 5;
    h.store
        .gc_step(
            GcBudget {
                max_events_per_step: 100,
                max_duration_ms: 5_000,
                max_total_events: ceiling,
            },
            NOW_SECS,
        )
        .unwrap();

    // After GC the store must be at or under the ceiling.
    // Count remaining events via scan_by_kind_time (kinds=&[] scans all kinds).
    let remaining: Vec<_> = h
        .store
        .scan_by_kind_time(&[], None, None, 1_000)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        remaining.len() <= ceiling,
        "store has {} events after gc, expected <= {}",
        remaining.len(),
        ceiling
    );
});

// ─── Test 3: LRU order — recently-read events survive ────────────────────────

for_each_backend!(lru_recently_read_events_survive, |h: &mut StoreHarness| {
    // Insert 5 events and record their ids.
    let ids: Vec<_> = (0..5u64)
        .map(|i| {
            let ev = h.make_event(ALICE_HEX, 1, 1_000 + i);
            let id = ev.id_bytes().expect("fixture: valid hex");
            h.insert_raw(ev, "wss://t/", (1_000 + i) * 1_000);
            id
        })
        .collect();

    // Bump the access counter for the LAST two events by reading them.
    // These are the most recently read → they must survive eviction.
    let hot_id_a = ids[3];
    let hot_id_b = ids[4];
    h.store.get_by_id(&hot_id_a).unwrap();
    h.store.get_by_id(&hot_id_b).unwrap();

    // Ceiling = 2 → only 2 events may survive. Evict 3.
    h.store
        .gc_step(
            GcBudget {
                max_events_per_step: 100,
                max_duration_ms: 5_000,
                max_total_events: 2,
            },
            NOW_SECS,
        )
        .unwrap();

    // The two events we read (hot) must still be present.
    h.assert_present(&hot_id_a);
    h.assert_present(&hot_id_b);

    // The un-read events (cold — ids[0..3]) must all be absent.
    // Exactly 2 survive (the hot ones), so all 3 cold ones are gone.
    let cold_ids = &ids[0..3];
    for cold_id in cold_ids {
        h.assert_absent(cold_id);
    }
});

// ─── Test 4: pinned events are never evicted ─────────────────────────────────

for_each_backend!(lru_pinned_events_survive, |h: &mut StoreHarness| {
    // Insert 4 events.
    let ids: Vec<_> = (0..4u64)
        .map(|i| {
            let ev = h.make_event(ALICE_HEX, 1, 1_000 + i);
            let id = ev.id_bytes().expect("fixture: valid hex");
            h.insert_raw(ev, "wss://t/", (1_000 + i) * 1_000);
            id
        })
        .collect();

    // Pin the first two events.  Do NOT read them (so their LRU seq stays old).
    let claimer = ClaimerId(99);
    h.store.register_view_cover(claimer, 10).unwrap();
    h.store.claim(claimer, &[ids[0], ids[1]]).unwrap();

    // Ceiling = 1 → would normally evict 3, but pinned events can't be evicted.
    // Only the 2 un-pinned events are candidates; both should be evicted.
    let report = h
        .store
        .gc_step(
            GcBudget {
                max_events_per_step: 100,
                max_duration_ms: 5_000,
                max_total_events: 1,
            },
            NOW_SECS,
        )
        .unwrap();

    // Pinned events survive regardless of access order.
    h.assert_present(&ids[0]);
    h.assert_present(&ids[1]);

    // The two un-pinned events were evicted.
    assert_eq!(
        report.lru_evicted, 2,
        "expected 2 lru_evicted (the un-pinned events), got {}",
        report.lru_evicted
    );
});

// ─── Test 5: relay index is consistent after eviction ────────────────────────

for_each_backend!(
    lru_relay_index_consistent_after_eviction,
    |h: &mut StoreHarness| {
        // Insert 4 events from the same relay.
        let relay = "wss://test-relay/";
        let ids: Vec<_> = (0..4u64)
            .map(|i| {
                let ev = h.make_event(ALICE_HEX, 1, 1_000 + i);
                let id = ev.id_bytes().expect("fixture: valid hex");
                h.insert_raw(ev, relay, (1_000 + i) * 1_000);
                id
            })
            .collect();

        // Read the last 2 events so they are the most recently accessed.
        h.store.get_by_id(&ids[2]).unwrap();
        h.store.get_by_id(&ids[3]).unwrap();

        // Ceiling = 2 → evict ids[0] and ids[1].
        h.store
            .gc_step(
                GcBudget {
                    max_events_per_step: 100,
                    max_duration_ms: 5_000,
                    max_total_events: 2,
                },
                NOW_SECS,
            )
            .unwrap();

        // MemEventStore supports list_events_seen_on; LMDB returns NotSupported.
        // Only verify the index on the mem backend.
        match h.store.list_events_seen_on(relay) {
            Ok(present_ids) => {
                // ids[0] and ids[1] were evicted → must NOT appear in the index.
                assert!(
                    !present_ids.contains(&ids[0]),
                    "evicted event ids[0] must not appear in relay index"
                );
                assert!(
                    !present_ids.contains(&ids[1]),
                    "evicted event ids[1] must not appear in relay index"
                );
                // ids[2] and ids[3] survived.
                assert!(
                    present_ids.contains(&ids[2]),
                    "surviving event ids[2] must appear in relay index"
                );
                assert!(
                    present_ids.contains(&ids[3]),
                    "surviving event ids[3] must appear in relay index"
                );
            }
            // LMDB returns NotSupported — skip the index assertions.
            Err(_) => {}
        }
    }
);

// ─── Test 6: clock test — gc_step uses now_secs, not wall clock ───────────────
//
// Insert an event that expires at `exp = 10`.
// Pass `now_secs = 5` (before expiry): event must NOT be reaped.
// Pass `now_secs = 11` (after expiry): event MUST be reaped.
// Verifies D7 compliance — no SystemTime::now() in gc_step.

for_each_backend!(gc_step_uses_passed_now_secs, |h: &mut StoreHarness| {
    let ev = h.make_event_with_tags(
        ALICE_HEX,
        1,
        1,
        vec![vec!["expiration".to_string(), "10".to_string()]],
    );
    let id = ev.id_bytes().expect("fixture: valid hex");
    // received_at_ms=1 (before expiry), so not rejected on arrival.
    h.insert_raw(ev, "wss://t/", 1);
    h.assert_present(&id);

    // GC with now_secs = 5 (before expiry): event must survive.
    let report = h
        .store
        .gc_step(
            GcBudget {
                max_events_per_step: 100,
                max_duration_ms: 5_000,
                max_total_events: usize::MAX,
            },
            5, // before exp=10
        )
        .unwrap();
    assert_eq!(report.expired_reaped, 0, "event must not expire at now_secs=5");
    h.assert_present(&id);

    // GC with now_secs = 11 (after expiry): event must be reaped.
    let report2 = h
        .store
        .gc_step(
            GcBudget {
                max_events_per_step: 100,
                max_duration_ms: 5_000,
                max_total_events: usize::MAX,
            },
            11, // after exp=10
        )
        .unwrap();
    assert_eq!(report2.expired_reaped, 1, "event must expire at now_secs=11");
    h.assert_absent(&id);
});

// ─── Test 7: no LRU eviction when store is at or under ceiling ───────────────

for_each_backend!(lru_no_eviction_under_ceiling, |h: &mut StoreHarness| {
    // Insert 3 events; ceiling = 5 → nothing to evict.
    for i in 0..3u64 {
        let ev = h.make_event(ALICE_HEX, 1, 1_000 + i);
        h.insert_raw(ev, "wss://t/", (1_000 + i) * 1_000);
    }

    let report = h
        .store
        .gc_step(
            GcBudget {
                max_events_per_step: 100,
                max_duration_ms: 5_000,
                max_total_events: 5,
            },
            NOW_SECS,
        )
        .unwrap();

    assert_eq!(report.lru_evicted, 0, "must not evict when under ceiling");
});
