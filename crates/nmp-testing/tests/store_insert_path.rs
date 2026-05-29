//! §2.1 Insert API single path tests.
//!
//! See `docs/design/lmdb/tests/insert.md` §2.1.

use nmp_core::store::{InsertOutcome, RejectReason};
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX};

for_each_backend!(insert_returns_inserted_outcome, |h: &mut StoreHarness| {
    let ev = h.make_event(ALICE_HEX, 1, 1_000_000);
    let id = ev.id_bytes().expect("fixture: valid hex");
    let outcome = h.insert_raw(ev, "wss://t/", 1_000_000_000);
    assert!(
        matches!(outcome, InsertOutcome::Inserted { .. }),
        "expected Inserted, got {outcome:?}"
    );
    h.assert_present(&id);
});

for_each_backend!(insert_ephemeral_not_stored, |h: &mut StoreHarness| {
    // Kind 20000 is ephemeral (NIP-16).
    let ev = h.make_event(ALICE_HEX, 20_000, 1_000_000);
    let id = ev.id_bytes().expect("fixture: valid hex");
    let outcome = h.insert_raw(ev, "wss://t/", 1_000_000_000);
    assert!(
        matches!(outcome, InsertOutcome::Ephemeral { .. }),
        "expected Ephemeral, got {outcome:?}"
    );
    // Ephemeral events must not be stored.
    h.assert_absent(&id);
});

for_each_backend!(insert_malformed_sig_rejected, |h: &mut StoreHarness| {
    // A structurally invalid event: id is too short (not valid 64-hex).
    // id_bytes() returns None for malformed input — the test intentionally
    // exercises the malformed path, so we use unwrap_or here.
    let mut ev = h.make_event(ALICE_HEX, 1, 1_000_000);
    ev.id = "deadbeef".to_string(); // not 64 hex chars — intentionally malformed
    assert!(ev.id_bytes().is_none(), "malformed id must return None");
    let outcome = h.insert_raw(ev, "wss://t/", 1_000_000_000);
    assert!(
        matches!(
            outcome,
            InsertOutcome::Rejected {
                reason: RejectReason::Malformed(_),
                ..
            }
        ),
        "expected Rejected(Malformed), got {outcome:?}"
    );
    // The store cannot have stored anything — the event was rejected before any write.
});

for_each_backend!(insert_get_by_id_round_trip, |h: &mut StoreHarness| {
    let ev = h.make_event(ALICE_HEX, 1, 1_000_000);
    let id = ev.id_bytes().expect("fixture: valid hex");
    let content = ev.content.clone();
    h.insert_raw(ev, "wss://t/", 1_000_000_000);
    let stored = h.store.get_by_id(&id).unwrap().expect("should be present");
    assert_eq!(stored.raw.content, content);
});

for_each_backend!(
    insert_provenance_created_on_insert,
    |h: &mut StoreHarness| {
        let ev = h.make_event(ALICE_HEX, 1, 1_000_000);
        let id = ev.id_bytes().expect("fixture: valid hex");
        h.insert_raw(ev, "wss://a/", 1_000_000_000);
        let prov = h.store.provenance_for(&id).unwrap();
        assert_eq!(prov.len(), 1);
        assert_eq!(prov[0].relay_url, "wss://a/");
        assert!(prov[0].primary);
    }
);
