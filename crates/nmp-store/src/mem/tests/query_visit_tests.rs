use std::ops::ControlFlow;

use crate::types::{RawEvent, StoreQuery, VerifiedEvent};
use crate::{EventStore, MemEventStore};

fn raw_event(id_hex: &str, pubkey_hex: &str, kind: u32, created_at: u64) -> RawEvent {
    RawEvent {
        id: id_hex.to_string(),
        pubkey: pubkey_hex.to_string(),
        created_at,
        kind,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    }
}

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

/// BLOCK-2 regression: `peek_by_id` must NOT advance the LRU access counter.
///
/// `get_by_id` stamps the LRU counter (bumps `access_seq` and updates
/// `access_index`) so the GC can identify least-recently-used events.
/// `peek_by_id` is a pure read that MUST leave both `access_seq` and
/// `access_index` unchanged — replay paths that use it must not bias
/// GC victim selection.
#[test]
fn peek_by_id_does_not_advance_lru_access_counter() {
    let store = MemEventStore::new();
    let id_hex = "03".repeat(32);
    let pk_hex = "04".repeat(32);

    // Helper: decode a 64-char hex string into [u8; 32].
    fn hex_to_bytes(hex: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let hi = u8::from_str_radix(std::str::from_utf8(&[chunk[0]]).unwrap(), 16).unwrap();
            let lo = u8::from_str_radix(std::str::from_utf8(&[chunk[1]]).unwrap(), 16).unwrap();
            out[i] = (hi << 4) | lo;
        }
        out
    }

    let ev = raw_event(&id_hex, &pk_hex, 1, 5_000_000);
    store
        .insert(unchecked(ev), &"wss://r/".to_string(), 5_000_000)
        .unwrap();

    // After insert the access counter is stamped once.
    let seq_after_insert = store.state.lock().unwrap().access_seq;
    assert!(seq_after_insert > 0, "insert must stamp access_seq");

    let id_bytes = hex_to_bytes(&id_hex);

    // get_by_id MUST advance the counter.
    let result = store.get_by_id(&id_bytes).unwrap();
    assert!(result.is_some(), "get_by_id must return the event");
    let seq_after_get = store.state.lock().unwrap().access_seq;
    assert!(
        seq_after_get > seq_after_insert,
        "get_by_id must advance access_seq ({seq_after_insert} → {seq_after_get})"
    );

    // peek_by_id must NOT advance the counter.
    let result = store.peek_by_id(&id_bytes).unwrap();
    assert!(result.is_some(), "peek_by_id must return the event");
    let seq_after_peek = store.state.lock().unwrap().access_seq;
    assert_eq!(
        seq_after_peek, seq_after_get,
        "peek_by_id must NOT advance access_seq (stayed at {seq_after_get})"
    );

    // Verify access_index is also unchanged after peek.
    let idx_after_peek = store
        .state
        .lock()
        .unwrap()
        .access_index
        .get(&id_hex)
        .copied()
        .expect("access_index entry must exist");
    assert_eq!(
        idx_after_peek, seq_after_get,
        "peek_by_id must not update access_index[id] (expected {seq_after_get}, got {idx_after_peek})"
    );
}
