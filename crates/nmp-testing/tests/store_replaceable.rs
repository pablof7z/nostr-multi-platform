//! §2.4 Replaceable supersession tests (kinds 0, 3, 10000–19999).
//!
//! See `docs/design/lmdb/tests/insert.md` §2.4.

use nmp_core::store::InsertOutcome;
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX};

for_each_backend!(newer_replaceable_supersedes_older, |h: &mut StoreHarness| {
    // Insert kind:0 v1 (older).
    let ev1 = h.make_event(ALICE_HEX, 0, 1_000);
    let id1 = ev1.id_bytes();
    let o1 = h.insert_raw(ev1, "wss://t/", 1_000_000);
    assert!(matches!(o1, InsertOutcome::Inserted { .. }), "{o1:?}");

    // Insert kind:0 v2 (newer created_at).
    let ev2 = h.make_event(ALICE_HEX, 0, 2_000);
    let id2 = ev2.id_bytes();
    let o2 = h.insert_raw(ev2, "wss://t/", 2_000_000);
    assert!(
        matches!(o2, InsertOutcome::Replaced { .. }),
        "expected Replaced, got {o2:?}"
    );

    // Old event gone, new event present.
    h.assert_absent(&id1);
    h.assert_present(&id2);

    // Scan returns only the new event.
    let author = nmp_testing::store_harness::ALICE_PUBKEY;
    let results: Vec<_> = h.store
        .scan_by_author_kind(&author, &[0], None, None, 10)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].raw.id_bytes(), id2);
});

for_each_backend!(older_replaceable_is_superseded, |h: &mut StoreHarness| {
    // Insert kind:0 v2 (newer) first.
    let ev2 = h.make_event(ALICE_HEX, 0, 2_000);
    let id2 = ev2.id_bytes();
    h.insert_raw(ev2, "wss://t/", 2_000_000);

    // Insert kind:0 v1 (older created_at) — should be Superseded.
    let ev1 = h.make_event(ALICE_HEX, 0, 1_000);
    let id1 = ev1.id_bytes();
    let o1 = h.insert_raw(ev1, "wss://t/", 1_000_000);
    assert!(
        matches!(o1, InsertOutcome::Superseded { .. }),
        "expected Superseded, got {o1:?}"
    );

    // v1 was not inserted; v2 still present.
    h.assert_absent(&id1);
    h.assert_present(&id2);
});

for_each_backend!(replaceable_tiebreak_by_id, |h: &mut StoreHarness| {
    // Two kind:0 events with the same created_at — lexicographically smaller id wins.
    // Create events with specific ids to control the tiebreak.
    let id_large = "f".repeat(64); // lexicographically large
    let id_small = "0".repeat(64); // lexicographically small

    let ev_large = h.make_event_with_id(&id_large, ALICE_HEX, 0, 1_000);
    let ev_small = h.make_event_with_id(&id_small, ALICE_HEX, 0, 1_000);

    h.insert_raw(ev_large.clone(), "wss://t/", 1_000_000);
    let o = h.insert_raw(ev_small.clone(), "wss://t/", 1_000_000);

    // Smaller id should win (replace the larger).
    assert!(
        matches!(o, InsertOutcome::Replaced { .. }),
        "smaller id should replace larger id at same timestamp, got {o:?}"
    );

    let winner_id = ev_small.id_bytes();
    let loser_id = ev_large.id_bytes();
    h.assert_present(&winner_id);
    h.assert_absent(&loser_id);
});

for_each_backend!(different_kinds_not_replaceable_against_each_other, |h: &mut StoreHarness| {
    // kind:0 and kind:3 are separate replaceable slots — they should not interfere.
    let ev0 = h.make_event(ALICE_HEX, 0, 1_000);
    let id0 = ev0.id_bytes();
    let ev3 = h.make_event(ALICE_HEX, 3, 1_000);
    let id3 = ev3.id_bytes();

    h.insert_raw(ev0, "wss://t/", 1_000_000);
    h.insert_raw(ev3, "wss://t/", 1_000_000);

    h.assert_present(&id0);
    h.assert_present(&id3);
});
