//! #1518 relay×kind presence-index parity tests for `MemEventStore`.
//!
//! Mirrors `relay_index_tests.rs` but targets `relay_kind_coverage` /
//! `relay_kind_count` so the in-memory backend is held to the same contract as
//! the LMDB backend: insert recording, duplicate de-dup, multi-kind coverage,
//! the NIP-04/17/59 privacy gate, delete cleanup, supersession, and GC.

use std::collections::{BTreeSet, HashSet};

use crate::types::{DeleteFilter, GcBudget, RawEvent, VerifiedEvent};
use crate::{EventStore, MemEventStore};

fn unchecked(raw: RawEvent) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(raw)
}

fn make_event(id_byte: u8, kind: u32, created_at: u64) -> RawEvent {
    RawEvent {
        id: format!("{id_byte:02x}").repeat(32),
        pubkey: "01".repeat(32),
        created_at,
        kind,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    }
}

const RELAY_A: &str = "wss://a.relay.example.com";
const RELAY_B: &str = "wss://b.relay.example.com";

/// Inserting a kind-1 event from relay A records (A, 1).
#[test]
fn insert_records_relay_kind() {
    let store = MemEventStore::new();
    store
        .insert(
            unchecked(make_event(0x01, 1, 1000)),
            &RELAY_A.to_string(),
            1_000_000,
        )
        .unwrap();

    assert_eq!(store.relay_kind_coverage(RELAY_A).unwrap(), vec![1]);
    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);
    assert_eq!(store.relay_kind_count(RELAY_A, 7).unwrap(), 0);
}

/// Re-delivering the same event from the same relay must not double-count.
#[test]
fn duplicate_does_not_double_count() {
    let store = MemEventStore::new();
    let ev = make_event(0x02, 1, 1000);
    store
        .insert(unchecked(ev.clone()), &RELAY_A.to_string(), 1_000_000)
        .unwrap();
    store
        .insert(unchecked(ev), &RELAY_A.to_string(), 1_000_001)
        .unwrap();

    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);
}

/// The same event from a second relay counts under each relay independently.
#[test]
fn duplicate_from_second_relay() {
    let store = MemEventStore::new();
    let ev = make_event(0x03, 1, 1000);
    store
        .insert(unchecked(ev.clone()), &RELAY_A.to_string(), 1_000_000)
        .unwrap();
    store
        .insert(unchecked(ev), &RELAY_B.to_string(), 1_000_001)
        .unwrap();

    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);
    assert_eq!(store.relay_kind_count(RELAY_B, 1).unwrap(), 1);
}

/// Distinct kinds appear in coverage ascending.
#[test]
fn distinct_kinds_coverage() {
    let store = MemEventStore::new();
    store
        .insert(
            unchecked(make_event(0x04, 1, 1000)),
            &RELAY_A.to_string(),
            1_000_000,
        )
        .unwrap();
    store
        .insert(
            unchecked(make_event(0x05, 7, 1001)),
            &RELAY_A.to_string(),
            1_000_001,
        )
        .unwrap();
    store
        .insert(
            unchecked(make_event(0x06, 30000, 1002)),
            &RELAY_A.to_string(),
            1_000_002,
        )
        .unwrap();

    assert_eq!(
        store.relay_kind_coverage(RELAY_A).unwrap(),
        vec![1, 7, 30000]
    );
}

/// Privacy gate: NIP-04/17/59 kinds never enter the index.
#[test]
fn privacy_gate_excludes_private_kinds() {
    let store = MemEventStore::new();
    store
        .insert(
            unchecked(make_event(0x07, 1, 1000)),
            &RELAY_A.to_string(),
            1_000_000,
        )
        .unwrap();
    let mut idb = 0x10u8;
    for k in [4u32, 13, 14, 15, 1059, 1060] {
        store
            .insert(
                unchecked(make_event(idb, k, 2000)),
                &RELAY_A.to_string(),
                2_000_000,
            )
            .unwrap();
        idb += 1;
        assert_eq!(
            store.relay_kind_count(RELAY_A, k).unwrap(),
            0,
            "private kind {k} must never be counted"
        );
    }
    assert_eq!(store.relay_kind_coverage(RELAY_A).unwrap(), vec![1]);
}

/// Defense-in-depth: even if a stale/private row exists in the derived map,
/// read-side coverage and count must not expose it.
#[test]
fn read_backstop_hides_stale_private_relay_kind_entries() {
    let store = MemEventStore::new();
    {
        let mut st = store.lock().unwrap();
        let relay = st.relay_kind.entry(RELAY_A.to_string()).or_default();
        relay.entry(1).or_default().insert("11".repeat(32));
        relay
            .entry(1059)
            .or_insert_with(BTreeSet::new)
            .insert("22".repeat(32));
    }

    assert_eq!(store.relay_kind_coverage(RELAY_A).unwrap(), vec![1]);
    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);
    assert_eq!(store.relay_kind_count(RELAY_A, 1059).unwrap(), 0);
}

/// Deleting an event removes its relay×kind entry.
#[test]
fn delete_removes_relay_kind() {
    let store = MemEventStore::new();
    let ev = make_event(0x20, 1, 1000);
    let id = crate::types::hex_to_event_id(&ev.id).unwrap();
    store
        .insert(unchecked(ev), &RELAY_A.to_string(), 1_000_000)
        .unwrap();
    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);

    store
        .delete_by_filter(DeleteFilter::ByIds(vec![id]))
        .unwrap();

    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 0);
    assert!(store.relay_kind_coverage(RELAY_A).unwrap().is_empty());
}

/// Replaceable supersession (kind:0) keeps exactly one event on the relay.
#[test]
fn replaceable_supersession_updates_relay_kind() {
    let store = MemEventStore::new();
    let pk = "aa".repeat(32);
    let old_ev = RawEvent {
        id: "31".repeat(32),
        pubkey: pk.clone(),
        created_at: 100,
        kind: 0,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };
    let new_ev = RawEvent {
        id: "32".repeat(32),
        pubkey: pk,
        created_at: 200,
        kind: 0,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };
    store
        .insert(unchecked(old_ev), &RELAY_A.to_string(), 100_000)
        .unwrap();
    store
        .insert(unchecked(new_ev), &RELAY_A.to_string(), 200_000)
        .unwrap();

    assert_eq!(store.relay_kind_count(RELAY_A, 0).unwrap(), 1);
    assert_eq!(store.relay_kind_coverage(RELAY_A).unwrap(), vec![0]);
}

/// GC LRU eviction removes the evicted event from the relay×kind index.
#[test]
fn gc_lru_eviction_removes_relay_kind() {
    let store = MemEventStore::new();
    store
        .insert(
            unchecked(make_event(0x40, 1, 1000)),
            &RELAY_A.to_string(),
            1_000_000,
        )
        .unwrap();
    store
        .insert(
            unchecked(make_event(0x41, 1, 1001)),
            &RELAY_A.to_string(),
            1_000_001,
        )
        .unwrap();
    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 2);

    let budget = GcBudget {
        max_events_per_step: 100,
        max_duration_ms: 10_000,
        max_total_events: 1,
    };
    store
        .gc_step_with_pins(budget, 2_000, &HashSet::new())
        .unwrap();

    assert_eq!(store.relay_kind_count(RELAY_A, 1).unwrap(), 1);
}

/// An unknown relay reports empty coverage and zero counts.
#[test]
fn unknown_relay_is_empty() {
    let store = MemEventStore::new();
    assert!(store
        .relay_kind_coverage("wss://never.example.com")
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .relay_kind_count("wss://never.example.com", 1)
            .unwrap(),
        0
    );
}
