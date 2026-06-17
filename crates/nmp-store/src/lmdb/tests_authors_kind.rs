//! LMDB-backend `StoreQuery::AuthorsKind` multi-author query parity tests.
//!
//! Mirrors `mem/tests/authors_kind_tests.rs` — same scenarios, same expected
//! outcomes. Split from `tests.rs` to stay under the 500-LOC hard cap.

#![cfg(feature = "lmdb-backend")]

use std::collections::BTreeSet;

use nostr::prelude::*;

use crate::types::StoreQuery;
use crate::EventStore;

use super::test_fixtures::{open_tmp, verified};

#[test]
fn authors_kind_newest_first_across_authors() {
    let (store, _dir) = open_tmp();
    let keys_a = Keys::generate();
    let keys_b = Keys::generate();

    let mut pk_a = [0u8; 32];
    pk_a.copy_from_slice(keys_a.public_key().to_bytes().as_slice());
    let mut pk_b = [0u8; 32];
    pk_b.copy_from_slice(keys_b.public_key().to_bytes().as_slice());

    // Author A: kind=1, created_at 1000, 2000, 3000
    for ts in [1000u64, 2000, 3000] {
        let ev = EventBuilder::text_note(format!("a-{ts}"))
            .custom_created_at(Timestamp::from_secs(ts))
            .sign_with_keys(&keys_a)
            .unwrap();
        let raw: crate::types::RawEvent = serde_json::from_str(&ev.try_as_json().unwrap()).unwrap();
        store.insert(verified(raw), &"wss://r/".into(), ts).unwrap();
    }
    // Author B: kind=1, created_at 1500, 2500
    for ts in [1500u64, 2500] {
        let ev = EventBuilder::text_note(format!("b-{ts}"))
            .custom_created_at(Timestamp::from_secs(ts))
            .sign_with_keys(&keys_b)
            .unwrap();
        let raw: crate::types::RawEvent = serde_json::from_str(&ev.try_as_json().unwrap()).unwrap();
        store.insert(verified(raw), &"wss://r/".into(), ts).unwrap();
    }

    let mut authors = BTreeSet::new();
    authors.insert(pk_a);
    authors.insert(pk_b);

    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![1],
        since: None,
        until: None,
    };

    let results = store.query(&q, 100).unwrap();
    assert_eq!(results.len(), 5, "must return all 5 events");
    let timestamps: Vec<u64> = results.iter().map(|e| e.raw.created_at).collect();
    assert_eq!(timestamps, vec![3000, 2500, 2000, 1500, 1000], "must be newest-first across authors");
}

#[test]
fn authors_kind_limit_respected() {
    let (store, _dir) = open_tmp();
    let keys_a = Keys::generate();
    let keys_b = Keys::generate();

    let mut pk_a = [0u8; 32];
    pk_a.copy_from_slice(keys_a.public_key().to_bytes().as_slice());
    let mut pk_b = [0u8; 32];
    pk_b.copy_from_slice(keys_b.public_key().to_bytes().as_slice());

    for i in 0..10u64 {
        let ev = EventBuilder::text_note(format!("a-{i}"))
            .custom_created_at(Timestamp::from_secs(1000 + i))
            .sign_with_keys(&keys_a)
            .unwrap();
        let raw: crate::types::RawEvent = serde_json::from_str(&ev.try_as_json().unwrap()).unwrap();
        store.insert(verified(raw), &"wss://r/".into(), 1000 + i).unwrap();

        let ev2 = EventBuilder::text_note(format!("b-{i}"))
            .custom_created_at(Timestamp::from_secs(2000 + i))
            .sign_with_keys(&keys_b)
            .unwrap();
        let raw2: crate::types::RawEvent = serde_json::from_str(&ev2.try_as_json().unwrap()).unwrap();
        store.insert(verified(raw2), &"wss://r/".into(), 2000 + i).unwrap();
    }

    let mut authors = BTreeSet::new();
    authors.insert(pk_a);
    authors.insert(pk_b);

    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![1],
        since: None,
        until: None,
    };

    let results = store.query(&q, 5).unwrap();
    assert_eq!(results.len(), 5, "limit must cap at 5");
    // Newest-first
    for w in results.windows(2) {
        assert!(w[0].raw.created_at >= w[1].raw.created_at, "must be newest-first");
    }
}

#[test]
fn authors_kind_since_until_bounds() {
    let (store, _dir) = open_tmp();
    let keys_a = Keys::generate();
    let keys_b = Keys::generate();

    let mut pk_a = [0u8; 32];
    pk_a.copy_from_slice(keys_a.public_key().to_bytes().as_slice());
    let mut pk_b = [0u8; 32];
    pk_b.copy_from_slice(keys_b.public_key().to_bytes().as_slice());

    for ts in [100u64, 200, 300, 400, 500] {
        let ev = EventBuilder::text_note(format!("a-{ts}"))
            .custom_created_at(Timestamp::from_secs(ts))
            .sign_with_keys(&keys_a)
            .unwrap();
        let raw: crate::types::RawEvent = serde_json::from_str(&ev.try_as_json().unwrap()).unwrap();
        store.insert(verified(raw), &"wss://r/".into(), ts).unwrap();

        let ev2 = EventBuilder::text_note(format!("b-{}", ts + 50))
            .custom_created_at(Timestamp::from_secs(ts + 50))
            .sign_with_keys(&keys_b)
            .unwrap();
        let raw2: crate::types::RawEvent = serde_json::from_str(&ev2.try_as_json().unwrap()).unwrap();
        store.insert(verified(raw2), &"wss://r/".into(), ts + 50).unwrap();
    }

    let mut authors = BTreeSet::new();
    authors.insert(pk_a);
    authors.insert(pk_b);

    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![1],
        since: Some(200),
        until: Some(350),
    };

    let results = store.query(&q, 100).unwrap();
    for ev in &results {
        assert!(
            ev.raw.created_at >= 200 && ev.raw.created_at <= 350,
            "event at {} must be within since/until bounds",
            ev.raw.created_at
        );
    }
    for w in results.windows(2) {
        assert!(w[0].raw.created_at >= w[1].raw.created_at, "must be newest-first");
    }
}
