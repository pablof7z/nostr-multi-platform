//! §2.5 Parameterized replaceable tests (kinds 30000–39999).
//!
//! See `docs/design/lmdb/tests/insert.md` §2.5.

use nmp_core::store::InsertOutcome;
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX, ALICE_PUBKEY};

for_each_backend!(
    newer_param_replaceable_supersedes_older,
    |h: &mut StoreHarness| {
        let ev1 = h.make_event_with_tags(
            ALICE_HEX,
            30_023,
            1_000,
            vec![vec!["d".to_string(), "foo".to_string()]],
        );
        let id1 = ev1.id_bytes();
        let o1 = h.insert_raw(ev1, "wss://t/", 1_000_000);
        assert!(matches!(o1, InsertOutcome::Inserted { .. }), "{o1:?}");

        let ev2 = h.make_event_with_tags(
            ALICE_HEX,
            30_023,
            2_000,
            vec![vec!["d".to_string(), "foo".to_string()]],
        );
        let id2 = ev2.id_bytes();
        let o2 = h.insert_raw(ev2, "wss://t/", 2_000_000);
        assert!(matches!(o2, InsertOutcome::Replaced { .. }), "{o2:?}");

        h.assert_absent(&id1);
        h.assert_present(&id2);

        let retrieved = h
            .store
            .get_param_replaceable(&ALICE_PUBKEY, 30_023, b"foo")
            .unwrap();
        assert!(
            retrieved.is_some(),
            "get_param_replaceable should return the newer event"
        );
        assert_eq!(retrieved.unwrap().raw.id_bytes(), id2);
    }
);

for_each_backend!(different_dtag_separate_slots, |h: &mut StoreHarness| {
    let ev_foo = h.make_event_with_tags(
        ALICE_HEX,
        30_023,
        1_000,
        vec![vec!["d".to_string(), "foo".to_string()]],
    );
    let id_foo = ev_foo.id_bytes();

    let ev_bar = h.make_event_with_tags(
        ALICE_HEX,
        30_023,
        1_000,
        vec![vec!["d".to_string(), "bar".to_string()]],
    );
    let id_bar = ev_bar.id_bytes();

    h.insert_raw(ev_foo, "wss://t/", 1_000_000);
    h.insert_raw(ev_bar, "wss://t/", 1_000_000);

    // Both present independently.
    h.assert_present(&id_foo);
    h.assert_present(&id_bar);

    let r_foo = h
        .store
        .get_param_replaceable(&ALICE_PUBKEY, 30_023, b"foo")
        .unwrap();
    let r_bar = h
        .store
        .get_param_replaceable(&ALICE_PUBKEY, 30_023, b"bar")
        .unwrap();
    assert_eq!(r_foo.unwrap().raw.id_bytes(), id_foo);
    assert_eq!(r_bar.unwrap().raw.id_bytes(), id_bar);
});

for_each_backend!(
    different_kind_same_dtag_no_collision,
    |h: &mut StoreHarness| {
        // kind:30023 and kind:30024 with d=foo are separate slots.
        let ev_23 = h.make_event_with_tags(
            ALICE_HEX,
            30_023,
            1_000,
            vec![vec!["d".to_string(), "foo".to_string()]],
        );
        let id_23 = ev_23.id_bytes();

        let ev_24 = h.make_event_with_tags(
            ALICE_HEX,
            30_024,
            2_000,
            vec![vec!["d".to_string(), "foo".to_string()]],
        );
        let id_24 = ev_24.id_bytes();

        h.insert_raw(ev_23, "wss://t/", 1_000_000);
        h.insert_raw(ev_24, "wss://t/", 2_000_000);

        h.assert_present(&id_23);
        h.assert_present(&id_24);

        let r23 = h
            .store
            .get_param_replaceable(&ALICE_PUBKEY, 30_023, b"foo")
            .unwrap();
        let r24 = h
            .store
            .get_param_replaceable(&ALICE_PUBKEY, 30_024, b"foo")
            .unwrap();
        assert_eq!(r23.unwrap().raw.id_bytes(), id_23);
        assert_eq!(r24.unwrap().raw.id_bytes(), id_24);
    }
);

for_each_backend!(
    older_param_replaceable_is_superseded,
    |h: &mut StoreHarness| {
        // Insert newer first, then try to insert older — should be Superseded.
        let ev2 = h.make_event_with_tags(
            ALICE_HEX,
            30_023,
            2_000,
            vec![vec!["d".to_string(), "foo".to_string()]],
        );
        let id2 = ev2.id_bytes();
        h.insert_raw(ev2, "wss://t/", 2_000_000);

        let ev1 = h.make_event_with_tags(
            ALICE_HEX,
            30_023,
            1_000,
            vec![vec!["d".to_string(), "foo".to_string()]],
        );
        let id1 = ev1.id_bytes();
        let o = h.insert_raw(ev1, "wss://t/", 1_000_000);
        assert!(matches!(o, InsertOutcome::Superseded { .. }), "{o:?}");

        h.assert_absent(&id1);
        h.assert_present(&id2);
    }
);
