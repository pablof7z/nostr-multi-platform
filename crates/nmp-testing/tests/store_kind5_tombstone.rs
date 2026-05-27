//! §2.6 Kind:5 self-delete + tombstone persistence tests.
//!
//! See `docs/design/lmdb/tests/insert.md` §2.6.

use nmp_core::store::{InsertOutcome, TombstoneOrigin};
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX};

for_each_backend!(
    kind5_self_delete_tombstones_target,
    |h: &mut StoreHarness| {
        // Insert a kind:1 by Alice.
        let kind1 = h.make_event(ALICE_HEX, 1, 1_000);
        let kind1_id = kind1.id_bytes();
        let kind1_id_hex = kind1.id.clone();
        h.insert_raw(kind1, "wss://t/", 1_000_000);
        h.assert_present(&kind1_id);

        // Insert kind:5 by Alice referencing the kind:1 via `e` tag.
        let kind5 = h.make_event_with_tags(
            ALICE_HEX,
            5,
            2_000,
            vec![vec!["e".to_string(), kind1_id_hex]],
        );
        h.insert_raw(kind5, "wss://t/", 2_000_000);

        // kind:1 should be gone from primary.
        h.assert_absent(&kind1_id);

        // Tombstone row should exist.
        h.assert_tombstoned(&kind1_id);
        let tombs = h.store.tombstones_for(&kind1_id).unwrap();
        assert_eq!(tombs.len(), 1);
        assert_eq!(tombs[0].origin, TombstoneOrigin::Kind5);
        assert_eq!(tombs[0].target_id, kind1_id);
    }
);

for_each_backend!(kind5_tombstone_blocks_reinsert, |h: &mut StoreHarness| {
    // Insert and delete kind:1.
    let kind1 = h.make_event(ALICE_HEX, 1, 1_000);
    let kind1_id = kind1.id_bytes();
    let kind1_id_hex = kind1.id.clone();
    let kind1_clone = kind1.clone();
    h.insert_raw(kind1, "wss://t/", 1_000_000);

    let kind5 = h.make_event_with_tags(
        ALICE_HEX,
        5,
        2_000,
        vec![vec!["e".to_string(), kind1_id_hex]],
    );
    h.insert_raw(kind5, "wss://t/", 2_000_000);
    h.assert_absent(&kind1_id);

    // Attempt to re-insert the same kind:1 — should be Tombstoned.
    let o = h.insert_raw(kind1_clone, "wss://b/", 3_000_000);
    assert!(
        matches!(
            o,
            InsertOutcome::Tombstoned {
                origin: TombstoneOrigin::Kind5,
                ..
            }
        ),
        "re-insert should be Tombstoned, got {o:?}"
    );
    h.assert_absent(&kind1_id);
});

for_each_backend!(kind5_stored_as_regular_event, |h: &mut StoreHarness| {
    // The kind:5 event itself should be stored in primary.
    let kind1 = h.make_event(ALICE_HEX, 1, 1_000);
    let kind1_id_hex = kind1.id.clone();
    h.insert_raw(kind1, "wss://t/", 1_000_000);

    let kind5 = h.make_event_with_tags(
        ALICE_HEX,
        5,
        2_000,
        vec![vec!["e".to_string(), kind1_id_hex]],
    );
    let kind5_id = kind5.id_bytes();
    h.insert_raw(kind5, "wss://t/", 2_000_000);

    h.assert_present(&kind5_id);
});
