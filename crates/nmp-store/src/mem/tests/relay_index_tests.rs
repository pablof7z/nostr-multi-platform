//! V-52 relay-origin reverse index tests.
//!
//! These tests verify that `relay_index` is maintained correctly under inserts,
//! duplicate re-deliveries, delete_by_filter, and GC, and that
//! `list_events_seen_on` returns the expected event ids.

use crate::types::{DeleteFilter, RawEvent, VerifiedEvent};
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

/// Basic invariant: inserting an event from relay A registers it in the
/// relay_index under relay A.
#[test]
fn insert_registers_event_in_relay_index() {
    let store = MemEventStore::new();
    let ev = make_event(0x01, 1, 1000);
    let id_hex = ev.id.clone();
    store
        .insert(unchecked(ev), &RELAY_A.to_string(), 1_000_000)
        .unwrap();

    let ids = store.list_events_seen_on(RELAY_A).unwrap();
    let id_bytes: Vec<[u8; 32]> = ids;
    let expected = crate::types::hex_to_event_id(&id_hex).unwrap();
    assert!(
        id_bytes.contains(&expected),
        "inserted event must appear in relay A's index"
    );
}

/// Re-delivering the same event from relay B must add B to provenance AND
/// register the event in relay B's index.
#[test]
fn duplicate_delivery_from_second_relay_registers_in_both_indexes() {
    let store = MemEventStore::new();
    let ev = make_event(0x02, 1, 1000);
    let id_hex = ev.id.clone();
    store
        .insert(unchecked(ev.clone()), &RELAY_A.to_string(), 1_000_000)
        .unwrap();
    store
        .insert(unchecked(ev), &RELAY_B.to_string(), 1_000_001)
        .unwrap();

    let ids_a = store.list_events_seen_on(RELAY_A).unwrap();
    let ids_b = store.list_events_seen_on(RELAY_B).unwrap();
    let expected = crate::types::hex_to_event_id(&id_hex).unwrap();
    assert!(ids_a.contains(&expected), "must be in relay A index");
    assert!(ids_b.contains(&expected), "must be in relay B index");
}

/// Relay A events must NOT appear in relay B's index.
#[test]
fn relay_index_is_relay_scoped() {
    let store = MemEventStore::new();
    let ev_a = make_event(0x03, 1, 1000);
    let ev_b = make_event(0x04, 1, 1001);
    let id_a = crate::types::hex_to_event_id(&ev_a.id).unwrap();
    let id_b = crate::types::hex_to_event_id(&ev_b.id).unwrap();
    store
        .insert(unchecked(ev_a), &RELAY_A.to_string(), 1_000_000)
        .unwrap();
    store
        .insert(unchecked(ev_b), &RELAY_B.to_string(), 1_000_001)
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

/// After delete_by_filter removes an event, it must disappear from the relay
/// index — no dangling references.
#[test]
fn delete_removes_event_from_relay_index() {
    let store = MemEventStore::new();
    let ev = make_event(0x05, 1, 1000);
    let id_bytes = crate::types::hex_to_event_id(&ev.id).unwrap();
    store
        .insert(unchecked(ev), &RELAY_A.to_string(), 1_000_000)
        .unwrap();

    // Verify it's there first.
    let ids_before = store.list_events_seen_on(RELAY_A).unwrap();
    assert!(
        ids_before.contains(&id_bytes),
        "must be present before delete"
    );

    // Delete by explicit id.
    store
        .delete_by_filter(DeleteFilter::ByIds(vec![id_bytes]))
        .unwrap();

    let ids_after = store.list_events_seen_on(RELAY_A).unwrap();
    assert!(
        !ids_after.contains(&id_bytes),
        "event must be gone from relay index after delete"
    );
}

/// An empty relay (no events from it) returns an empty list.
#[test]
fn list_events_seen_on_unknown_relay_returns_empty() {
    let store = MemEventStore::new();
    let ids = store
        .list_events_seen_on("wss://never-seen.example.com")
        .unwrap();
    assert!(ids.is_empty(), "unknown relay must return empty list");
}

/// Events from relay A inserted as replaceable (kind:0) — the new event
/// replaces the old one; the old event must leave the index, the new one must
/// be in it.
#[test]
fn replaceable_supersession_removes_old_event_from_relay_index() {
    let store = MemEventStore::new();
    let pk = "aa".repeat(32);
    let old_ev = RawEvent {
        id: "11".repeat(32),
        pubkey: pk.clone(),
        created_at: 100,
        kind: 0, // replaceable
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };
    let new_ev = RawEvent {
        id: "22".repeat(32),
        pubkey: pk,
        created_at: 200, // newer — must win
        kind: 0,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };
    let old_id = crate::types::hex_to_event_id(&old_ev.id).unwrap();
    let new_id = crate::types::hex_to_event_id(&new_ev.id).unwrap();

    store
        .insert(unchecked(old_ev), &RELAY_A.to_string(), 100_000)
        .unwrap();
    store
        .insert(unchecked(new_ev), &RELAY_A.to_string(), 200_000)
        .unwrap();

    let ids = store.list_events_seen_on(RELAY_A).unwrap();
    assert!(
        !ids.contains(&old_id),
        "replaced event must not be in index"
    );
    assert!(ids.contains(&new_id), "replacing event must be in index");
}
