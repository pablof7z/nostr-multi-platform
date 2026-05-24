//! LMDB-backend kind:5 deletion parity tests.
//!
//! Split out of `tests.rs` to fit the AGENTS.md 500-LOC hard cap. Covers:
//!   * Self-delete via `e`-tag — tombstone + re-delivery rejection.
//!   * Foreign target — silently skipped (parity with `mem/insert.rs:271`).
//!   * Foreign pre-tombstone — Alice's event must STILL Insert after Bob's
//!     premature kind:5 (regression for the `handle_kind5` `mark_deleted`
//!     bug — see commit history + ADR-0012 amendment).
//!   * Self pre-tombstone — author's own kind:5 before target arrives;
//!     target must surface as Tombstoned.
//!   * Single-source delete-by-relay via `DeleteFilter::ByRelayOnly`.

#![cfg(feature = "lmdb-backend")]

use crate::types::{InsertOutcome, RawEvent};
use crate::EventStore;

use super::test_fixtures::{open_tmp, signed_event, signed_event_with_keys, verified};

#[test]
fn kind5_self_delete_e_tag_writes_tombstone() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();

    let target = signed_event_with_keys(&keys, 1, 1000, "doomed", None);
    let target_id = target.id_bytes();
    store.insert(verified(target.clone()), &"wss://r/".into(), 1_000_000).unwrap();

    // kind:5 referencing target.
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(nostr::EventId::from_slice(&target_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .unwrap();
    let k5_json = k5.try_as_json().unwrap();
    let k5_raw: RawEvent = serde_json::from_str(&k5_json).unwrap();
    store.insert(verified(k5_raw), &"wss://r/".into(), 2_000_000).unwrap();

    // Tombstone present, target gone.
    let tombs = store.tombstones_for(&target_id).unwrap();
    assert!(!tombs.is_empty(), "tombstone must be recorded");
    assert!(store.get_by_id(&target_id).unwrap().is_none(), "target purged");

    // Re-delivery of the same target_id must surface as Tombstoned.
    let o = store.insert(verified(target), &"wss://r/".into(), 3_000_000).unwrap();
    assert!(matches!(o, InsertOutcome::Tombstoned { .. }), "got {o:?}");
}

#[test]
fn kind5_foreign_target_silently_skipped() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let alice = nostr::Keys::generate();
    let bob = nostr::Keys::generate();

    let alice_event = signed_event_with_keys(&alice, 1, 1000, "alice's note", None);
    let alice_id = alice_event.id_bytes();
    store.insert(verified(alice_event.clone()), &"wss://r/".into(), 1_000_000).unwrap();

    // Bob tries to delete Alice's event — must be silently skipped, NOT
    // rejected as InvalidDelete (parity with mem/insert.rs:271 continue).
    let foreign_k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(nostr::EventId::from_slice(&alice_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&bob)
        .unwrap();
    let json = foreign_k5.try_as_json().unwrap();
    let raw: RawEvent = serde_json::from_str(&json).unwrap();
    let o = store.insert(verified(raw), &"wss://r/".into(), 2_000_000).unwrap();
    // Bob's kind:5 itself is stored (it's a valid event of his), but the
    // foreign target is not deleted.
    assert!(
        matches!(o, InsertOutcome::Inserted { .. }),
        "foreign kind:5 must be stored, got {o:?}"
    );
    assert!(
        store.get_by_id(&alice_id).unwrap().is_some(),
        "alice's event must survive bob's foreign deletion attempt"
    );
}

/// Bob's kind:5 references Alice's event id BEFORE Alice's event arrives.
/// In Mem (`mem/insert.rs` foreign-pre-tombstone path), Alice's event MUST
/// still end up Inserted because the tombstone's deleter pubkey does not
/// match the event author. The LMDB adapter must match: dropping the NMP
/// tombstone in step 4 is not enough on its own; we must also not have
/// poisoned the fork's `deleted_ids` set in `handle_kind5`. This test is
/// the regression guard for that bug.
#[test]
fn kind5_foreign_pre_tombstone_then_event_arrives_inserts() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let alice = nostr::Keys::generate();
    let bob = nostr::Keys::generate();

    // Build Alice's event WITHOUT inserting it yet — we just need the id.
    let alice_event = signed_event_with_keys(&alice, 1, 1000, "alice's note", None);
    let alice_id = alice_event.id_bytes();

    // Bob ships kind:5 referencing Alice's id first.
    let bob_k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(nostr::EventId::from_slice(&alice_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(500))
        .sign_with_keys(&bob)
        .unwrap();
    let raw: RawEvent = serde_json::from_str(&bob_k5.try_as_json().unwrap()).unwrap();
    let o_k5 = store.insert(verified(raw), &"wss://r/".into(), 500_000).unwrap();
    assert!(
        matches!(o_k5, InsertOutcome::Inserted { .. }),
        "Bob's foreign kind:5 must store, got {o_k5:?}"
    );

    // Alice's actual event arrives next. Mem returns Inserted; LMDB MUST too.
    let o_alice = store
        .insert(verified(alice_event), &"wss://r/".into(), 1_000_000)
        .unwrap();
    assert!(
        matches!(o_alice, InsertOutcome::Inserted { .. }),
        "Alice's event must be Inserted (foreign pre-tombstone drops), got {o_alice:?}"
    );
    assert!(
        store.get_by_id(&alice_id).unwrap().is_some(),
        "Alice's event must be queryable after arrival"
    );
}

/// Author's own kind:5 references their own event id BEFORE the event
/// arrives. Then the target arrives — must surface as Tombstoned, matching
/// Mem.
#[test]
fn kind5_self_pre_tombstone_then_target_arrives_tombstoned() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let alice = nostr::Keys::generate();

    // Build Alice's event first (don't insert).
    let alice_event = signed_event_with_keys(&alice, 1, 1000, "alice's note", None);
    let alice_id = alice_event.id_bytes();

    // Alice ships her own kind:5 referencing her event id first.
    let self_k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(nostr::EventId::from_slice(&alice_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(500))
        .sign_with_keys(&alice)
        .unwrap();
    let raw: RawEvent = serde_json::from_str(&self_k5.try_as_json().unwrap()).unwrap();
    let o_k5 = store.insert(verified(raw), &"wss://r/".into(), 500_000).unwrap();
    assert!(
        matches!(o_k5, InsertOutcome::Inserted { .. }),
        "Alice's self-delete kind:5 must store, got {o_k5:?}"
    );

    // Target arrives later. Mem returns Tombstoned (applies=true: deleter
    // pubkey matches event pubkey).
    let o_target = store
        .insert(verified(alice_event), &"wss://r/".into(), 1_000_000)
        .unwrap();
    assert!(
        matches!(o_target, InsertOutcome::Tombstoned { .. }),
        "self-pre-tombstoned target must surface as Tombstoned, got {o_target:?}"
    );
    assert!(
        store.get_by_id(&alice_id).unwrap().is_none(),
        "tombstoned event must not be queryable"
    );
}

/// Exercise `delete_by_filter(ByRelayOnly)`: an event sourced from a single
/// relay must be purged when that relay is filtered; an event with multiple
/// provenance sources must survive (because it's not "exclusively" from
/// that relay).
#[test]
fn delete_by_filter_by_relay_only_purges_single_source_events() {
    use crate::types::DeleteFilter;
    let (store, _dir) = open_tmp();

    // Event A: single source on r1.
    let a = signed_event(1, 1000, "single-source", None);
    let a_id = a.id_bytes();
    store.insert(verified(a), &"wss://r1/".into(), 1_000_000).unwrap();

    // Event B: re-delivered from r1 and r2 — multi-source.
    let b = signed_event(1, 1001, "multi-source", None);
    let b_id = b.id_bytes();
    store.insert(verified(b.clone()), &"wss://r1/".into(), 1_000_000).unwrap();
    store.insert(verified(b), &"wss://r2/".into(), 1_000_001).unwrap();

    // Sanity: both present.
    assert!(store.get_by_id(&a_id).unwrap().is_some());
    assert!(store.get_by_id(&b_id).unwrap().is_some());

    // Delete everything sourced exclusively from r1.
    let removed = store
        .delete_by_filter(DeleteFilter::ByRelayOnly("wss://r1/".into()))
        .unwrap();
    assert_eq!(removed, 1, "exactly one event (A) had r1 as its only source");

    assert!(
        store.get_by_id(&a_id).unwrap().is_none(),
        "A (r1-only) must be purged"
    );
    assert!(
        store.get_by_id(&b_id).unwrap().is_some(),
        "B (r1+r2) must survive — not exclusively r1"
    );
}
