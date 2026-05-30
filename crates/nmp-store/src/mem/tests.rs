//! Unit tests for `MemEventStore` — P2 invariant checks.
//!
//! Integration tests using the full `StoreHarness` live in
//! `crates/nmp-testing/tests/store_*.rs`.

#[cfg(test)]
mod insert_tests {
    use crate::types::{InsertOutcome, RawEvent, VerifiedEvent};
    use crate::{EventStore, MemEventStore};

    fn unchecked(raw: RawEvent) -> VerifiedEvent {
        VerifiedEvent::from_raw_unchecked(raw)
    }

    /// P2: tombstone upsert must max-merge `deleted_at` and union sources.
    #[test]
    fn tombstone_max_merge_takes_newer_deleted_at() {
        let store = MemEventStore::new();
        let target_hex = "0a".repeat(32);
        let k5a_hex = "a1".repeat(32);
        let k5b_hex = "b2".repeat(32);

        // First kind:5 at t=100.
        let k5a = RawEvent {
            id: k5a_hex.clone(),
            pubkey: "01".repeat(32),
            created_at: 100,
            kind: 5,
            tags: vec![vec!["e".into(), target_hex.clone()]],
            content: String::new(),
            sig: "a".repeat(128),
        };
        store
            .insert(unchecked(k5a), &"wss://r1/".to_string(), 100_000)
            .unwrap();

        // Second kind:5 at t=200 (newer — should win for deleted_at).
        let k5b = RawEvent {
            id: k5b_hex.clone(),
            pubkey: "01".repeat(32),
            created_at: 200,
            kind: 5,
            tags: vec![vec!["e".into(), target_hex.clone()]],
            content: String::new(),
            sig: "a".repeat(128),
        };
        store
            .insert(unchecked(k5b), &"wss://r2/".to_string(), 200_000)
            .unwrap();

        let st = store.state.lock().unwrap();
        let tomb = st
            .tombstones
            .get(&target_hex)
            .expect("tombstone must exist");
        assert_eq!(
            tomb.deleted_at, 200,
            "max-merge must take the newer deleted_at"
        );
        assert!(
            tomb.sources.contains(&"wss://r1/".to_string()),
            "must union r1"
        );
        assert!(
            tomb.sources.contains(&"wss://r2/".to_string()),
            "must union r2"
        );
    }

    /// P2: same-id re-delivery for replaceable events must merge provenance,
    /// not count as a new supersession.
    #[test]
    fn replaceable_dup_id_merges_provenance() {
        let store = MemEventStore::new();
        let pk = "01".repeat(32);
        let id = "aa".repeat(32);
        let ev = RawEvent {
            id: id.clone(),
            pubkey: pk.clone(),
            created_at: 1000,
            kind: 0, // replaceable
            tags: vec![],
            content: String::new(),
            sig: "a".repeat(128),
        };

        let o1 = store
            .insert(unchecked(ev.clone()), &"wss://r1/".to_string(), 1_000_000)
            .unwrap();
        assert!(matches!(o1, InsertOutcome::Inserted { .. }));

        let o2 = store
            .insert(unchecked(ev), &"wss://r2/".to_string(), 2_000_000)
            .unwrap();
        assert!(
            matches!(o2, InsertOutcome::Duplicate { .. }),
            "re-delivery of same id must be Duplicate, got {o2:?}"
        );

        let id_bytes = [0xaau8; 32];
        let prov = store.provenance_for(&id_bytes).unwrap();
        assert_eq!(prov.len(), 2, "both relays must be in provenance");
    }
}

#[cfg(test)]
mod query_visit_tests {
    use std::ops::ControlFlow;

    use crate::types::{RawEvent, StoreQuery, VerifiedEvent};
    use crate::{EventStore, MemEventStore};

    fn unchecked(raw: RawEvent) -> VerifiedEvent {
        VerifiedEvent::from_raw_unchecked(raw)
    }

    /// Early-stop: with 10 000 matching events in the store, a visitor that
    /// breaks after the 10th must be invoked exactly 10 times — the scan stops
    /// without materializing the remaining 9 990 events.
    #[test]
    fn query_visit_stops_after_first_10_of_10000() {
        let store = MemEventStore::new();
        let pk = "01".repeat(32);
        for i in 0..10_000u64 {
            // Distinct ids; created_at descending so iteration order is stable.
            let id = format!("{i:064x}");
            let ev = RawEvent {
                id,
                pubkey: pk.clone(),
                created_at: 1_000_000 + i,
                kind: 1,
                tags: vec![],
                content: String::new(),
                sig: "a".repeat(128),
            };
            store
                .insert(unchecked(ev), &"wss://r/".to_string(), 1_000_000 + i)
                .unwrap();
        }

        let q = StoreQuery::KindTime {
            kinds: vec![1],
            since: None,
            until: None,
        };

        let mut visited = 0usize;
        store
            .query_visit(&q, 10_000, &mut |_ev| {
                visited += 1;
                if visited >= 10 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            })
            .unwrap();

        assert_eq!(visited, 10, "visitor must stop after exactly 10 events");
    }

    /// The Vec-returning `query` wrapper must yield the same events the visitor
    /// would see, capped by `limit`, newest-first.
    #[test]
    fn query_wrapper_matches_visit_order_and_limit() {
        let store = MemEventStore::new();
        let pk = "02".repeat(32);
        for i in 0..50u64 {
            let ev = RawEvent {
                id: format!("{i:064x}"),
                pubkey: pk.clone(),
                created_at: 2_000_000 + i,
                kind: 7,
                tags: vec![],
                content: String::new(),
                sig: "a".repeat(128),
            };
            store
                .insert(unchecked(ev), &"wss://r/".to_string(), 2_000_000 + i)
                .unwrap();
        }

        let q = StoreQuery::AuthorKind {
            author: [0x02u8; 32],
            kinds: vec![7],
            since: None,
            until: None,
        };

        let via_query = store.query(&q, 5).unwrap();
        assert_eq!(via_query.len(), 5, "limit must cap the result vec");
        // Newest-first: created_at strictly descending.
        for w in via_query.windows(2) {
            assert!(
                w[0].raw.created_at >= w[1].raw.created_at,
                "query results must be newest-first"
            );
        }
        assert_eq!(
            via_query[0].raw.created_at, 2_000_049,
            "first result must be the newest event"
        );
    }
}

/// V-52 relay-origin reverse index tests.
///
/// These tests verify that `relay_index` is maintained correctly under inserts,
/// duplicate re-deliveries, delete_by_filter, and GC, and that
/// `list_events_seen_on` returns the expected event ids.
#[cfg(test)]
mod relay_index_tests {
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
        store.insert(unchecked(ev), &RELAY_A.to_string(), 1_000_000).unwrap();

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
        store.insert(unchecked(ev.clone()), &RELAY_A.to_string(), 1_000_000).unwrap();
        store.insert(unchecked(ev), &RELAY_B.to_string(), 1_000_001).unwrap();

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
        store.insert(unchecked(ev_a), &RELAY_A.to_string(), 1_000_000).unwrap();
        store.insert(unchecked(ev_b), &RELAY_B.to_string(), 1_000_001).unwrap();

        let ids_a = store.list_events_seen_on(RELAY_A).unwrap();
        let ids_b = store.list_events_seen_on(RELAY_B).unwrap();
        assert!(ids_a.contains(&id_a), "event A must be in relay A index");
        assert!(!ids_a.contains(&id_b), "event B must NOT be in relay A index");
        assert!(ids_b.contains(&id_b), "event B must be in relay B index");
        assert!(!ids_b.contains(&id_a), "event A must NOT be in relay B index");
    }

    /// After delete_by_filter removes an event, it must disappear from the relay
    /// index — no dangling references.
    #[test]
    fn delete_removes_event_from_relay_index() {
        let store = MemEventStore::new();
        let ev = make_event(0x05, 1, 1000);
        let id_bytes = crate::types::hex_to_event_id(&ev.id).unwrap();
        store.insert(unchecked(ev), &RELAY_A.to_string(), 1_000_000).unwrap();

        // Verify it's there first.
        let ids_before = store.list_events_seen_on(RELAY_A).unwrap();
        assert!(ids_before.contains(&id_bytes), "must be present before delete");

        // Delete by explicit id.
        store.delete_by_filter(DeleteFilter::ByIds(vec![id_bytes])).unwrap();

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
        let ids = store.list_events_seen_on("wss://never-seen.example.com").unwrap();
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

        store.insert(unchecked(old_ev), &RELAY_A.to_string(), 100_000).unwrap();
        store.insert(unchecked(new_ev), &RELAY_A.to_string(), 200_000).unwrap();

        let ids = store.list_events_seen_on(RELAY_A).unwrap();
        assert!(!ids.contains(&old_id), "replaced event must not be in index");
        assert!(ids.contains(&new_id), "replacing event must be in index");
    }
}

#[cfg(test)]
mod gc_tests {
    use crate::types::ClaimerId;
    use crate::{EventStore, MemEventStore, StoreError};

    fn make_id(b: u8) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[0] = b;
        id
    }

    #[test]
    fn claim_idempotent_reclaim_does_not_count() {
        let store = MemEventStore::new();
        let c = ClaimerId(1);
        store.register_view_cover(c, 5).unwrap();
        let id = make_id(1);
        store.claim(c, &[id]).unwrap();
        store.claim(c, &[id]).unwrap();
        let st = store.state.lock().unwrap();
        assert_eq!(
            st.claims[&c].len(),
            1,
            "idempotent: re-claim must not add entry"
        );
    }

    #[test]
    fn claim_over_per_view_ceiling_returns_err() {
        let store = MemEventStore::new();
        let c = ClaimerId(2);
        store.register_view_cover(c, 2).unwrap();
        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
        let result = store.claim(c, &[make_id(3)]);
        assert!(
            matches!(result, Err(StoreError::OverPinned { .. })),
            "must return OverPinned when per-view ceiling exceeded"
        );
    }

    #[test]
    fn release_clears_all_pins() {
        let store = MemEventStore::new();
        let c = ClaimerId(3);
        store.register_view_cover(c, 100).unwrap();
        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
        store.release(c).unwrap();
        let st = store.state.lock().unwrap();
        assert!(
            !st.claims.contains_key(&c),
            "release must clear claimer's pins"
        );
    }
}
