//! Unit tests for `MemEventStore` — P2 invariant checks.
//!
//! Integration tests using the full `StoreHarness` live in
//! `crates/nmp-testing/tests/store_*.rs`.

#[cfg(test)]
mod insert_tests {
    use crate::store::types::{InsertOutcome, RawEvent, VerifiedEvent};
    use crate::store::{EventStore, MemEventStore};

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
        store.insert(unchecked(k5a), &"wss://r1/".to_string(), 100_000).unwrap();

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
        store.insert(unchecked(k5b), &"wss://r2/".to_string(), 200_000).unwrap();

        let st = store.state.lock().unwrap();
        let tomb = st.tombstones.get(&target_hex).expect("tombstone must exist");
        assert_eq!(tomb.deleted_at, 200, "max-merge must take the newer deleted_at");
        assert!(tomb.sources.contains(&"wss://r1/".to_string()), "must union r1");
        assert!(tomb.sources.contains(&"wss://r2/".to_string()), "must union r2");
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

        let o1 = store.insert(unchecked(ev.clone()), &"wss://r1/".to_string(), 1_000_000).unwrap();
        assert!(matches!(o1, InsertOutcome::Inserted { .. }));

        let o2 = store.insert(unchecked(ev), &"wss://r2/".to_string(), 2_000_000).unwrap();
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

    use crate::store::types::{RawEvent, StoreQuery, VerifiedEvent};
    use crate::store::{EventStore, MemEventStore};

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

#[cfg(test)]
mod gc_tests {
    use crate::store::types::ClaimerId;
    use crate::store::{EventStore, MemEventStore, StoreError};

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
        assert_eq!(st.claims[&c].len(), 1, "idempotent: re-claim must not add entry");
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
        assert!(!st.claims.contains_key(&c), "release must clear claimer's pins");
    }
}
