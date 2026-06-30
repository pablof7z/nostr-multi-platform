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
    assert_eq!(
        timestamps,
        vec![3000, 2500, 2000, 1500, 1000],
        "must be newest-first across authors"
    );
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
        store
            .insert(verified(raw), &"wss://r/".into(), 1000 + i)
            .unwrap();

        let ev2 = EventBuilder::text_note(format!("b-{i}"))
            .custom_created_at(Timestamp::from_secs(2000 + i))
            .sign_with_keys(&keys_b)
            .unwrap();
        let raw2: crate::types::RawEvent =
            serde_json::from_str(&ev2.try_as_json().unwrap()).unwrap();
        store
            .insert(verified(raw2), &"wss://r/".into(), 2000 + i)
            .unwrap();
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
        assert!(
            w[0].raw.created_at >= w[1].raw.created_at,
            "must be newest-first"
        );
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
        let raw2: crate::types::RawEvent =
            serde_json::from_str(&ev2.try_as_json().unwrap()).unwrap();
        store
            .insert(verified(raw2), &"wss://r/".into(), ts + 50)
            .unwrap();
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
        assert!(
            w[0].raw.created_at >= w[1].raw.created_at,
            "must be newest-first"
        );
    }
}

/// Insert a signed event of `kind` at `ts` for `keys` with distinct `nonce`
/// content (so events that share `(kind, ts)` still get distinct ids); returns
/// its hex id.
fn insert_kind(
    store: &super::LmdbEventStore,
    keys: &Keys,
    kind: u16,
    ts: u64,
    nonce: u32,
) -> String {
    let ev = EventBuilder::new(Kind::from(kind), format!("k{kind}-{ts}-{nonce}"))
        .custom_created_at(Timestamp::from_secs(ts))
        .sign_with_keys(keys)
        .unwrap();
    let raw: crate::types::RawEvent = serde_json::from_str(&ev.try_as_json().unwrap()).unwrap();
    store.insert(verified(raw), &"wss://r/".into(), ts).unwrap();
    ev.id.to_hex()
}

fn pk_bytes(keys: &Keys) -> crate::types::PubKey {
    let mut pk = [0u8; 32];
    pk.copy_from_slice(keys.public_key().to_bytes().as_slice());
    pk
}

/// Only the requested kinds are returned (parity with mem `authors_kind_filters_by_kind`).
#[test]
fn authors_kind_filters_by_kind() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    insert_kind(&store, &keys, 1, 1000, 0);
    insert_kind(&store, &keys, 3, 1001, 1);
    insert_kind(&store, &keys, 7, 1002, 2);

    let mut authors = BTreeSet::new();
    authors.insert(pk_bytes(&keys));

    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![1, 3],
        since: None,
        until: None,
    };
    let results = store.query(&q, 100).unwrap();
    assert_eq!(
        results.len(),
        2,
        "must only return kind 1 and kind 3 events"
    );
    for ev in &results {
        assert!(
            ev.raw.kind == 1 || ev.raw.kind == 3,
            "unexpected kind {}",
            ev.raw.kind
        );
    }
}

/// Empty authors set returns nothing — never a wildcard (parity with mem +
/// the empty-set contract on `StoreQuery::AuthorsKind`).
#[test]
fn authors_kind_empty_authors_returns_nothing() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    insert_kind(&store, &keys, 1, 1000, 0);

    let q = StoreQuery::AuthorsKind {
        authors: BTreeSet::new(),
        kinds: vec![1],
        since: None,
        until: None,
    };
    let results = store.query(&q, 100).unwrap();
    assert!(
        results.is_empty(),
        "empty authors must return nothing (not wildcard)"
    );
}

/// Single-author `AuthorKind` with an empty kind set returns nothing — same
/// positive-selection contract as `AuthorsKind` (the fork's `Filter` would
/// treat empty kinds as "any kind"; the store short-circuits to match mem).
#[test]
fn author_kind_empty_kinds_returns_nothing() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    insert_kind(&store, &keys, 1, 1000, 0);

    let q = StoreQuery::AuthorKind {
        author: pk_bytes(&keys),
        kinds: vec![],
        since: None,
        until: None,
    };
    let results = store.query(&q, 100).unwrap();
    assert!(
        results.is_empty(),
        "AuthorKind empty kinds must return nothing (not wildcard)"
    );
}

/// Empty kinds set returns nothing — never a wildcard. This is the LMDB-side
/// of the #1497 empty-set parity fix (the fork's `Filter` treats empty kinds as
/// "any kind"; the store short-circuits to match mem).
#[test]
fn authors_kind_empty_kinds_returns_nothing() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    insert_kind(&store, &keys, 1, 1000, 0);

    let mut authors = BTreeSet::new();
    authors.insert(pk_bytes(&keys));

    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![],
        since: None,
        until: None,
    };
    let results = store.query(&q, 100).unwrap();
    assert!(
        results.is_empty(),
        "empty kinds must return nothing (not wildcard)"
    );
}

/// Same event id inserted from two relays appears once (parity with mem).
#[test]
fn authors_kind_no_duplicate_event_ids() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let ev = EventBuilder::text_note("dup")
        .custom_created_at(Timestamp::from_secs(1000))
        .sign_with_keys(&keys)
        .unwrap();
    let raw: crate::types::RawEvent = serde_json::from_str(&ev.try_as_json().unwrap()).unwrap();
    store
        .insert(verified(raw.clone()), &"wss://r1/".into(), 1000)
        .unwrap();
    store
        .insert(verified(raw), &"wss://r2/".into(), 1000)
        .unwrap();

    let mut authors = BTreeSet::new();
    authors.insert(pk_bytes(&keys));

    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![1],
        since: None,
        until: None,
    };
    let results = store.query(&q, 100).unwrap();
    assert_eq!(results.len(), 1, "duplicate event id must appear only once");
}

/// Events with equal `created_at` are ordered by id ascending — the same
/// tie-break mem applies (`created_at desc, id asc`). Spans TWO authors and
/// TWO kinds all at the same timestamp, so the multi-author / multi-kind k-way
/// merge boundary is covered: EVERY limit prefix must be a prefix of the full
/// id-ascending order, proving the limit cut cannot pick a fork-dependent tie
/// member that mem would not.
#[test]
fn authors_kind_equal_created_at_tie_break_id_ascending() {
    let (store, _dir) = open_tmp();
    let keys_a = Keys::generate();
    let keys_b = Keys::generate();
    // Six events across two authors and two kinds, ALL at the same created_at →
    // the only ordering signal is the id tie-break.
    let mut ids = vec![
        insert_kind(&store, &keys_a, 1, 5000, 0),
        insert_kind(&store, &keys_a, 6, 5000, 1),
        insert_kind(&store, &keys_b, 1, 5000, 2),
        insert_kind(&store, &keys_b, 6, 5000, 3),
        insert_kind(&store, &keys_a, 1, 5000, 4),
        insert_kind(&store, &keys_b, 6, 5000, 5),
    ];
    ids.sort(); // expected full order: id ascending

    let mut authors = BTreeSet::new();
    authors.insert(pk_bytes(&keys_a));
    authors.insert(pk_bytes(&keys_b));
    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![1, 6],
        since: None,
        until: None,
    };

    let full = store.query(&q, 100).unwrap();
    let got: Vec<String> = full.iter().map(|e| e.raw.id.clone()).collect();
    assert_eq!(
        got, ids,
        "equal-created_at events must come out id-ascending across authors+kinds"
    );

    // EVERY limit prefix must equal the prefix of the full order — the limit
    // cut never picks a fork-dependent tie member mem would not.
    for n in 1..=ids.len() {
        let limited = store.query(&q, n).unwrap();
        let ids_n: Vec<String> = limited.iter().map(|e| e.raw.id.clone()).collect();
        assert_eq!(
            ids_n,
            ids[..n],
            "limit={n} must be the id-ascending prefix of the full order"
        );
    }
}
