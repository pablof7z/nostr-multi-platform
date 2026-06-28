//! #1518 relay×kind presence-index tests for the LMDB backend.
//!
//! Mirrors `tests_relay_index.rs`: every assertion targets an exact invariant of
//! the `nmp-relay-kind` projection — the derived `(relay, kind, event_id)` index
//! that lets the kernel ask which kinds a relay has served (and how many events
//! of each) without splitting store ownership.
//!
//! Covers: insert recording, duplicate de-dup, multi-relay, multi-kind coverage,
//! the NIP-04/17/59 privacy gate, delete cleanup, replaceable supersession, GC
//! LRU eviction, restart persistence, and one-time backfill on open.

#![cfg(feature = "lmdb-backend")]

use nostr::prelude::*;

use crate::types::{DeleteFilter, GcBudget};
use crate::{EventStore, LmdbEventStore};

use super::test_fixtures::{open_tmp, signed_event, signed_event_with_keys, verified};

const RELAY_A: &str = "wss://a.relay.example.com";
const RELAY_B: &str = "wss://b.relay.example.com";

/// Inserting an event of kind 1 from relay A records (A, 1) in the index:
/// coverage reports kind 1, count is 1.
#[test]
fn insert_records_relay_kind() {
    let (store, _dir) = open_tmp();
    let ev = signed_event(1, 1_000, "hello", None);
    store
        .insert(verified(ev), &RELAY_A.into(), 1_000_000)
        .unwrap();

    assert_eq!(
        store.relay_kind_coverage(RELAY_A).unwrap(),
        vec![1],
        "relay A must report kind 1 in its coverage"
    );
    assert_eq!(
        store.relay_kind_count(RELAY_A, 1).unwrap(),
        1,
        "relay A must have one kind-1 event"
    );
    // A kind never seen on the relay reports zero.
    assert_eq!(store.relay_kind_count(RELAY_A, 7).unwrap(), 0);
}

/// Re-delivering the SAME event from the same relay must not double-count it.
#[test]
fn duplicate_does_not_double_count() {
    let (store, _dir) = open_tmp();
    let ev = signed_event(1, 1_000, "dup", None);
    store
        .insert(verified(ev.clone()), &RELAY_A.into(), 1_000_000)
        .unwrap();
    store
        .insert(verified(ev), &RELAY_A.into(), 1_000_001)
        .unwrap();

    assert_eq!(
        store.relay_kind_count(RELAY_A, 1).unwrap(),
        1,
        "duplicate delivery from the same relay must count once"
    );
}

/// The same event delivered from a second relay counts under each relay
/// independently (presence is per-relay).
#[test]
fn duplicate_from_second_relay() {
    let (store, _dir) = open_tmp();
    let ev = signed_event(1, 1_000, "two-relays", None);
    store
        .insert(verified(ev.clone()), &RELAY_A.into(), 1_000_000)
        .unwrap();
    store
        .insert(verified(ev), &RELAY_B.into(), 1_000_001)
        .unwrap();

    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);
    assert_eq!(store.relay_kind_count(RELAY_B, 1).unwrap(), 1);
    assert_eq!(store.relay_kind_coverage(RELAY_A).unwrap(), vec![1]);
    assert_eq!(store.relay_kind_coverage(RELAY_B).unwrap(), vec![1]);
}

/// Distinct kinds on one relay appear in coverage, ascending.
#[test]
fn distinct_kinds_coverage() {
    let (store, _dir) = open_tmp();
    // kinds 1, 7, 30023 on relay A.
    store
        .insert(
            verified(signed_event(1, 1_000, "note", None)),
            &RELAY_A.into(),
            1_000_000,
        )
        .unwrap();
    store
        .insert(
            verified(signed_event(7, 1_001, "+", None)),
            &RELAY_A.into(),
            1_000_001,
        )
        .unwrap();
    store
        .insert(
            verified(signed_event(30023, 1_002, "long", Some("d1"))),
            &RELAY_A.into(),
            1_000_002,
        )
        .unwrap();

    assert_eq!(
        store.relay_kind_coverage(RELAY_A).unwrap(),
        vec![1, 7, 30023],
        "coverage must list distinct kinds ascending"
    );
    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);
    assert_eq!(store.relay_kind_count(RELAY_A, 7).unwrap(), 1);
    assert_eq!(store.relay_kind_count(RELAY_A, 30023).unwrap(), 1);
}

/// Privacy gate: NIP-04/17/59 kinds (4/13/14/15/1059/1060) never enter the
/// index — coverage omits them and their count is always zero.
#[test]
fn privacy_gate_excludes_private_kinds() {
    let (store, _dir) = open_tmp();
    // A non-private kind to prove the relay is otherwise indexed.
    store
        .insert(
            verified(signed_event(1, 1_000, "note", None)),
            &RELAY_A.into(),
            1_000_000,
        )
        .unwrap();
    for k in [4u32, 13, 14, 15, 1059, 1060] {
        store
            .insert(
                verified(signed_event(k, 2_000 + k as u64, "secret", None)),
                &RELAY_A.into(),
                2_000_000,
            )
            .unwrap();
        assert_eq!(
            store.relay_kind_count(RELAY_A, k).unwrap(),
            0,
            "private kind {k} must never be counted"
        );
    }
    assert_eq!(
        store.relay_kind_coverage(RELAY_A).unwrap(),
        vec![1],
        "coverage must contain only the non-private kind"
    );
}

/// Defense-in-depth: even if an old database or manual corruption leaves a
/// private relay-kind key behind, the read path must not expose it.
#[test]
fn read_backstop_hides_stale_private_relay_kind_entries() {
    let (store, _dir) = open_tmp();
    let inner = store.inner_for_test();
    let stale_private_id = [0x99u8; 32];
    let stale_public_id = [0x88u8; 32];
    let key = |kind: u32, id: &[u8; 32]| {
        let mut key = Vec::with_capacity(RELAY_A.len() + 1 + 4 + 32);
        key.extend_from_slice(RELAY_A.as_bytes());
        key.push(0);
        key.extend_from_slice(&kind.to_be_bytes());
        key.extend_from_slice(id);
        key
    };

    let mut txn = inner.env.write_txn().expect("write_txn");
    inner
        .relay_kind
        .put(&mut txn, &key(1, &stale_public_id), &[])
        .expect("put public stale key");
    inner
        .relay_kind
        .put(&mut txn, &key(1059, &stale_private_id), &[])
        .expect("put private stale key");
    txn.commit().expect("commit");

    assert_eq!(store.relay_kind_coverage(RELAY_A).unwrap(), vec![1]);
    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);
    assert_eq!(store.relay_kind_count(RELAY_A, 1059).unwrap(), 0);
}

/// Deleting an event removes its relay×kind entry — no dangling references.
#[test]
fn delete_removes_relay_kind() {
    let (store, _dir) = open_tmp();
    let ev = signed_event(1, 1_000, "to-delete", None);
    let id = ev.id_bytes().expect("id");
    store
        .insert(verified(ev), &RELAY_A.into(), 1_000_000)
        .unwrap();
    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);

    store
        .delete_by_filter(DeleteFilter::ByIds(vec![id]))
        .unwrap();

    assert_eq!(
        store.relay_kind_count(RELAY_A, 1).unwrap(),
        0,
        "deleted event must leave the relay×kind index"
    );
    assert!(
        store.relay_kind_coverage(RELAY_A).unwrap().is_empty(),
        "coverage must be empty after the only event is deleted"
    );
}

/// Replaceable supersession (kind:0): the superseded event leaves the index, the
/// replacing one enters it — the count stays 1, never 2.
#[test]
fn replaceable_supersession_updates_relay_kind() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let old_ev = signed_event_with_keys(&keys, 0, 100, "old", None);
    let new_ev = signed_event_with_keys(&keys, 0, 200, "new", None);
    store
        .insert(verified(old_ev), &RELAY_A.into(), 100_000)
        .unwrap();
    store
        .insert(verified(new_ev), &RELAY_A.into(), 200_000)
        .unwrap();

    assert_eq!(
        store.relay_kind_count(RELAY_A, 0).unwrap(),
        1,
        "supersession must keep exactly one kind-0 event on the relay"
    );
    assert_eq!(store.relay_kind_coverage(RELAY_A).unwrap(), vec![0]);
}

/// GC LRU eviction of an un-pinned event removes its relay×kind entry.
#[test]
fn gc_lru_eviction_removes_relay_kind() {
    let (store, _dir) = open_tmp();
    // Two distinct kind-1 events on relay A.
    store
        .insert(
            verified(signed_event(1, 1_000, "a", None)),
            &RELAY_A.into(),
            1_000_000,
        )
        .unwrap();
    store
        .insert(
            verified(signed_event(1, 1_001, "b", None)),
            &RELAY_A.into(),
            1_000_001,
        )
        .unwrap();
    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 2);

    // Force LRU eviction down to one event (no pins).
    let budget = GcBudget {
        max_events_per_step: 100,
        max_duration_ms: 10_000,
        max_total_events: 1,
    };
    store.gc_step(budget, 2_000).unwrap();

    assert_eq!(
        store.relay_kind_count(RELAY_A, 1).unwrap(),
        1,
        "LRU eviction must drop the evicted event from the relay×kind index"
    );
}

/// The index is persisted in a sub-db: closing and re-opening at the same path
/// preserves coverage + counts.
#[test]
fn relay_kind_survives_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let store = LmdbEventStore::open(dir.path()).expect("open");
        store
            .insert(
                verified(signed_event(1, 1_000, "persist", None)),
                &RELAY_A.into(),
                1_000_000,
            )
            .unwrap();
        store
            .insert(
                verified(signed_event(7, 1_001, "+", None)),
                &RELAY_A.into(),
                1_000_001,
            )
            .unwrap();
        assert_eq!(store.relay_kind_coverage(RELAY_A).unwrap(), vec![1, 7]);
    }
    let store = LmdbEventStore::open(dir.path()).expect("reopen");
    assert_eq!(
        store.relay_kind_coverage(RELAY_A).unwrap(),
        vec![1, 7],
        "relay×kind coverage must survive a store re-open"
    );
    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);
    assert_eq!(store.relay_kind_count(RELAY_A, 7).unwrap(), 1);
}

/// A pre-#1518 database has provenance rows + events but an empty relay-kind
/// sub-db. On open the backfill must reconstruct the index from provenance +
/// the event kinds — and must respect the privacy gate.
#[test]
fn relay_kind_backfilled_on_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let store = LmdbEventStore::open(dir.path()).expect("open");
        store
            .insert(
                verified(signed_event(1, 1_000, "backfill-me", None)),
                &RELAY_A.into(),
                1_000_000,
            )
            .unwrap();
        // A private kind that must stay out of the index even via backfill.
        store
            .insert(
                verified(signed_event(4, 1_001, "dm", None)),
                &RELAY_A.into(),
                1_000_001,
            )
            .unwrap();

        // Simulate a pre-#1518 store: wipe the relay-kind sub-db and the one-shot
        // backfill gate key so the next open re-runs the backfill.
        let inner = store.inner_for_test();
        let mut txn = inner.env.write_txn().expect("write_txn");
        inner.relay_kind.clear(&mut txn).expect("clear relay_kind");
        inner
            .domain_versions
            .delete(&mut txn, b"nmp-relay-kind".as_slice())
            .expect("delete gate key");
        txn.commit().expect("commit");

        // Confirm the simulated pre-#1518 state: relay-kind empty, provenance intact.
        assert!(
            store.relay_kind_coverage(RELAY_A).unwrap().is_empty(),
            "precondition: relay-kind index must be empty before backfill"
        );
    }

    // Re-open: the backfill reconstructs the relay-kind index from provenance.
    let store = LmdbEventStore::open(dir.path()).expect("reopen");
    assert_eq!(
        store.relay_kind_coverage(RELAY_A).unwrap(),
        vec![1],
        "backfill must reconstruct kind 1 and exclude the private kind 4"
    );
    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);
    assert_eq!(
        store.relay_kind_count(RELAY_A, 4).unwrap(),
        0,
        "backfill must honour the privacy gate for kind 4"
    );
}
