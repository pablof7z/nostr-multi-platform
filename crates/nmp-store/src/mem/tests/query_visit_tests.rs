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
