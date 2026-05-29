//! §2.7 Foreign kind:5 ignored tests.
//!
//! See `docs/design/lmdb/tests/insert.md` §2.7.

use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX, BOB_HEX};

for_each_backend!(
    foreign_kind5_does_not_delete_target,
    |h: &mut StoreHarness| {
        // Insert kind:1 by Alice.
        let kind1 = h.make_event(ALICE_HEX, 1, 1_000);
        let kind1_id = kind1.id_bytes().expect("fixture: valid hex");
        let kind1_id_hex = kind1.id.clone();
        h.insert_raw(kind1, "wss://t/", 1_000_000);

        // Insert kind:5 by Bob referencing Alice's kind:1.
        let kind5_bob =
            h.make_event_with_tags(BOB_HEX, 5, 2_000, vec![vec!["e".to_string(), kind1_id_hex]]);
        h.insert_raw(kind5_bob, "wss://t/", 2_000_000);

        // Alice's kind:1 must still be present — Bob can't delete it.
        h.assert_present(&kind1_id);

        // No tombstone should have been written for Alice's event.
        let tombs = h.store.tombstones_for(&kind1_id).unwrap();
        assert!(
            tombs.is_empty(),
            "foreign kind:5 must not create a tombstone"
        );
    }
);

for_each_backend!(foreign_kind5_is_stored, |h: &mut StoreHarness| {
    // Bob's kind:5 should be stored as a regular event (other clients may want it).
    let kind1 = h.make_event(ALICE_HEX, 1, 1_000);
    let kind1_id_hex = kind1.id.clone();
    h.insert_raw(kind1, "wss://t/", 1_000_000);

    let kind5_bob =
        h.make_event_with_tags(BOB_HEX, 5, 2_000, vec![vec!["e".to_string(), kind1_id_hex]]);
    let kind5_id = kind5_bob.id_bytes().expect("fixture: valid hex");
    h.insert_raw(kind5_bob, "wss://t/", 2_000_000);

    h.assert_present(&kind5_id);
});
