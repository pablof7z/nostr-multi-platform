//! V-52 (#969) relay-origin reverse-index tests for the LMDB backend.
//!
//! Mirrors `mem/tests.rs::relay_index_tests` so the two backends are held to the
//! same `list_events_seen_on` contract, plus two LMDB-specific tests:
//!
//!   * `relay_index_survives_reopen` — the index is persisted (sub-db), not an
//!     in-memory projection.
//!   * `relay_index_backfilled_from_provenance_on_open` — a pre-V-52 database
//!     (provenance present, relay-index sub-db empty) is backfilled exactly once
//!     on open.
//!
//! Strict TDD: every assertion targets the exact invariant the fix enforces.

#![cfg(feature = "lmdb-backend")]

use nostr::prelude::*;

use crate::types::DeleteFilter;
use crate::{EventStore, LmdbEventStore};

use super::test_fixtures::{open_tmp, signed_event, signed_event_with_keys, verified};

const RELAY_A: &str = "wss://a.relay.example.com";
const RELAY_B: &str = "wss://b.relay.example.com";

/// Inserting an event from relay A registers it under relay A.
#[test]
fn insert_registers_event_in_relay_index() {
    let (store, _dir) = open_tmp();
    let ev = signed_event(1, 1_000, "hello", None);
    let id = ev.id_bytes().expect("id");
    store
        .insert(verified(ev), &RELAY_A.into(), 1_000_000)
        .unwrap();

    let ids = store.list_events_seen_on(RELAY_A).unwrap();
    assert!(
        ids.contains(&id),
        "inserted event must appear in relay A index"
    );
}

/// Re-delivering the same event from relay B registers it in both indexes.
#[test]
fn duplicate_delivery_from_second_relay_registers_in_both_indexes() {
    let (store, _dir) = open_tmp();
    let ev = signed_event(1, 1_000, "dup", None);
    let id = ev.id_bytes().expect("id");
    store
        .insert(verified(ev.clone()), &RELAY_A.into(), 1_000_000)
        .unwrap();
    store
        .insert(verified(ev), &RELAY_B.into(), 1_000_001)
        .unwrap();

    let ids_a = store.list_events_seen_on(RELAY_A).unwrap();
    let ids_b = store.list_events_seen_on(RELAY_B).unwrap();
    assert!(ids_a.contains(&id), "must be in relay A index");
    assert!(ids_b.contains(&id), "must be in relay B index");
}

/// Relay A events must NOT appear in relay B's index, and vice versa.
#[test]
fn relay_index_is_relay_scoped() {
    let (store, _dir) = open_tmp();
    let ev_a = signed_event(1, 1_000, "a", None);
    let ev_b = signed_event(1, 1_001, "b", None);
    let id_a = ev_a.id_bytes().expect("id");
    let id_b = ev_b.id_bytes().expect("id");
    store
        .insert(verified(ev_a), &RELAY_A.into(), 1_000_000)
        .unwrap();
    store
        .insert(verified(ev_b), &RELAY_B.into(), 1_000_001)
        .unwrap();

    let ids_a = store.list_events_seen_on(RELAY_A).unwrap();
    let ids_b = store.list_events_seen_on(RELAY_B).unwrap();
    assert!(ids_a.contains(&id_a), "event A must be in relay A index");
    assert!(
        !ids_a.contains(&id_b),
        "event B must NOT be in relay A index"
    );
    assert!(ids_b.contains(&id_b), "event B must be in relay B index");
    assert!(
        !ids_b.contains(&id_a),
        "event A must NOT be in relay B index"
    );
}

/// After `delete_by_filter` removes an event, it must disappear from the relay
/// index — no dangling references.
#[test]
fn delete_removes_event_from_relay_index() {
    let (store, _dir) = open_tmp();
    let ev = signed_event(1, 1_000, "to-delete", None);
    let id = ev.id_bytes().expect("id");
    store
        .insert(verified(ev), &RELAY_A.into(), 1_000_000)
        .unwrap();

    let before = store.list_events_seen_on(RELAY_A).unwrap();
    assert!(before.contains(&id), "must be present before delete");

    store
        .delete_by_filter(DeleteFilter::ByIds(vec![id]))
        .unwrap();

    let after = store.list_events_seen_on(RELAY_A).unwrap();
    assert!(
        !after.contains(&id),
        "event must be gone from relay index after delete"
    );
}

/// `DeleteFilter::ByRelayOnly` uses the relay index to find candidates but must
/// only delete events seen on EXACTLY that relay.  An event also present on a
/// second relay must survive; both must then be absent / present in the index.
#[test]
fn delete_by_relay_only_uses_index_and_respects_provenance() {
    let (store, _dir) = open_tmp();
    // `only_a`: seen on relay A only → must be deleted by ByRelayOnly(A).
    let only_a = signed_event(1, 1_000, "only-a", None);
    let only_a_id = only_a.id_bytes().expect("id");
    // `both`: seen on A and B → must survive ByRelayOnly(A).
    let both = signed_event(1, 1_001, "both", None);
    let both_id = both.id_bytes().expect("id");

    store
        .insert(verified(only_a), &RELAY_A.into(), 1_000_000)
        .unwrap();
    store
        .insert(verified(both.clone()), &RELAY_A.into(), 1_000_001)
        .unwrap();
    store
        .insert(verified(both), &RELAY_B.into(), 1_000_002)
        .unwrap();

    let removed = store
        .delete_by_filter(DeleteFilter::ByRelayOnly(RELAY_A.to_string()))
        .unwrap();
    assert_eq!(
        removed, 1,
        "only the relay-A-exclusive event must be deleted"
    );

    // `only_a` is gone from the store and from relay A's index.
    assert!(store.get_by_id(&only_a_id).unwrap().is_none());
    assert!(!store
        .list_events_seen_on(RELAY_A)
        .unwrap()
        .contains(&only_a_id));

    // `both` survives and remains indexed on both relays.
    assert!(store.get_by_id(&both_id).unwrap().is_some());
    assert!(store
        .list_events_seen_on(RELAY_A)
        .unwrap()
        .contains(&both_id));
    assert!(store
        .list_events_seen_on(RELAY_B)
        .unwrap()
        .contains(&both_id));
}

/// An unknown relay returns an empty list.
#[test]
fn list_events_seen_on_unknown_relay_returns_empty() {
    let (store, _dir) = open_tmp();
    let ids = store
        .list_events_seen_on("wss://never-seen.example.com")
        .unwrap();
    assert!(ids.is_empty(), "unknown relay must return empty list");
}

/// Replaceable supersession (kind:0) on relay A: the superseded event leaves the
/// index, the new one enters it.
#[test]
fn replaceable_supersession_removes_old_event_from_relay_index() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let old_ev = signed_event_with_keys(&keys, 0, 100, "old", None);
    let new_ev = signed_event_with_keys(&keys, 0, 200, "new", None);
    let old_id = old_ev.id_bytes().expect("id");
    let new_id = new_ev.id_bytes().expect("id");

    store
        .insert(verified(old_ev), &RELAY_A.into(), 100_000)
        .unwrap();
    store
        .insert(verified(new_ev), &RELAY_A.into(), 200_000)
        .unwrap();

    let ids = store.list_events_seen_on(RELAY_A).unwrap();
    assert!(
        !ids.contains(&old_id),
        "replaced event must not be in index"
    );
    assert!(ids.contains(&new_id), "replacing event must be in index");
}

/// The index is persisted in a sub-db: closing and re-opening the store at the
/// same path must preserve `list_events_seen_on`.
#[test]
fn relay_index_survives_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let id;
    {
        let store = LmdbEventStore::open(dir.path()).expect("open");
        let ev = signed_event(1, 1_000, "persist", None);
        id = ev.id_bytes().expect("id");
        store
            .insert(verified(ev), &RELAY_A.into(), 1_000_000)
            .unwrap();
        // Sanity before reopen.
        assert!(store.list_events_seen_on(RELAY_A).unwrap().contains(&id));
    }
    // Re-open the same physical database.
    let store = LmdbEventStore::open(dir.path()).expect("reopen");
    let ids = store.list_events_seen_on(RELAY_A).unwrap();
    assert!(
        ids.contains(&id),
        "relay index entry must survive a store re-open"
    );
}

/// A pre-V-52 database has provenance rows but an empty relay-index sub-db. On
/// open the backfill must reconstruct the index from provenance.
///
/// We simulate the pre-V-52 state by clearing the relay-index sub-db AND the
/// backfill gate key, then re-opening so the backfill runs.
#[test]
fn relay_index_backfilled_from_provenance_on_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let id;
    {
        let store = LmdbEventStore::open(dir.path()).expect("open");
        let ev = signed_event(1, 1_000, "backfill-me", None);
        id = ev.id_bytes().expect("id");
        store
            .insert(verified(ev), &RELAY_A.into(), 1_000_000)
            .unwrap();

        // Simulate a pre-V-52 store: wipe the relay-index sub-db and the
        // one-shot backfill gate key so the next open re-runs the backfill.
        let inner = store.inner_for_test();
        let mut txn = inner.env.write_txn().expect("write_txn");
        inner
            .relay_index
            .clear(&mut txn)
            .expect("clear relay_index");
        inner
            .domain_versions
            .delete(&mut txn, b"nmp-relay-index".as_slice())
            .expect("delete gate key");
        txn.commit().expect("commit");

        // Confirm the simulated pre-V-52 state: index empty, provenance intact.
        assert!(
            store.list_events_seen_on(RELAY_A).unwrap().is_empty(),
            "precondition: relay index must be empty before backfill"
        );
        assert!(
            !store.provenance_for(&id).unwrap().is_empty(),
            "precondition: provenance must still carry the relay"
        );
    }

    // Re-open: the backfill reconstructs the relay index from provenance.
    let store = LmdbEventStore::open(dir.path()).expect("reopen");
    let ids = store.list_events_seen_on(RELAY_A).unwrap();
    assert!(
        ids.contains(&id),
        "backfill must reconstruct the relay index from provenance"
    );
}
