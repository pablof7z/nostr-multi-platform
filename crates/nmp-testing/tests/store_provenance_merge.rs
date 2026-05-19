//! §2.3 Duplicate id → provenance merge tests.
//!
//! See `docs/design/lmdb/tests/insert.md` §2.3.

use nmp_core::store::InsertOutcome;
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX};

for_each_backend!(duplicate_merges_provenance_keeps_earliest, |h: &mut StoreHarness| {
    let ev = h.make_event(ALICE_HEX, 1, 1_000_000);
    let id = ev.id_bytes();
    let ev2 = ev.clone();

    let o1 = h.insert_raw(ev, "wss://a/", 1_000);
    let o2 = h.insert_raw(ev2, "wss://b/", 2_000);

    assert!(matches!(o1, InsertOutcome::Inserted { .. }), "first insert: {o1:?}");
    assert!(
        matches!(o2, InsertOutcome::Duplicate { sources_after: 2, .. }),
        "second insert: {o2:?}"
    );

    let prov = h.store.provenance_for(&id).unwrap();
    assert_eq!(prov.len(), 2, "expected 2 provenance entries");

    // Primary is the first-seen relay.
    let primary = prov.iter().find(|e| e.primary).expect("primary entry missing");
    assert_eq!(primary.relay_url, "wss://a/");
    assert_eq!(primary.first_seen_ms, 1_000, "earliest first_seen_ms must be preserved");
});

for_each_backend!(same_relay_duplicate_updates_last_seen, |h: &mut StoreHarness| {
    let ev = h.make_event(ALICE_HEX, 1, 1_000_000);
    let id = ev.id_bytes();
    let ev2 = ev.clone();

    h.insert_raw(ev, "wss://a/", 1_000);
    h.insert_raw(ev2, "wss://a/", 5_000);

    let prov = h.store.provenance_for(&id).unwrap();
    assert_eq!(prov.len(), 1, "same relay should not create duplicate provenance entry");
    assert_eq!(prov[0].first_seen_ms, 1_000, "first_seen_ms should remain earliest");
    assert_eq!(prov[0].last_seen_ms, 5_000, "last_seen_ms should be updated");
});

for_each_backend!(provenance_sources_after_count, |h: &mut StoreHarness| {
    let ev = h.make_event(ALICE_HEX, 1, 1_000_000);
    let id = ev.id_bytes();

    let relays = ["wss://r1/", "wss://r2/", "wss://r3/"];
    for (i, relay) in relays.iter().enumerate() {
        let ev_clone = ev.clone();
        let outcome = h.insert_raw(ev_clone, relay, (i + 1) as u64 * 1000);
        let expected_sources = (i + 1) as u32;
        match outcome {
            InsertOutcome::Inserted { sources_after, .. } => {
                assert_eq!(sources_after, 1, "first insert should report 1 source");
            }
            InsertOutcome::Duplicate { sources_after, .. } => {
                assert_eq!(sources_after, expected_sources, "sources_after mismatch at relay {relay}");
            }
            other => panic!("unexpected outcome {other:?}"),
        }
    }

    let prov = h.store.provenance_for(&id).unwrap();
    assert_eq!(prov.len(), 3);
});
