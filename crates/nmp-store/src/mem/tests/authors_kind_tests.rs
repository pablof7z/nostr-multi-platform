//! `StoreQuery::AuthorsKind` tests — multi-author query primitive.

use std::collections::BTreeSet;

use crate::types::{RawEvent, StoreQuery, VerifiedEvent};
use crate::{EventStore, MemEventStore};

fn unchecked(raw: RawEvent) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(raw)
}

fn make_pk(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn pk_hex(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

/// AuthorsKind returns events from all specified authors, newest-first.
#[test]
fn authors_kind_newest_first_across_authors() {
    let store = MemEventStore::new();
    let pk_a = make_pk(0xaa);
    let pk_b = make_pk(0xbb);

    // Author A: kind=1, created_at 1000, 2000, 3000
    for (i, ts) in [1000u64, 2000, 3000].iter().enumerate() {
        let ev = RawEvent {
            id: format!("aa{i:062x}"),
            pubkey: pk_hex(0xaa),
            created_at: *ts,
            kind: 1,
            tags: vec![],
            content: format!("a-{ts}"),
            sig: "a".repeat(128),
        };
        store
            .insert(unchecked(ev), &"wss://r/".into(), *ts)
            .unwrap();
    }
    // Author B: kind=1, created_at 1500, 2500
    for (i, ts) in [1500u64, 2500].iter().enumerate() {
        let ev = RawEvent {
            id: format!("bb{i:062x}"),
            pubkey: pk_hex(0xbb),
            created_at: *ts,
            kind: 1,
            tags: vec![],
            content: format!("b-{ts}"),
            sig: "b".repeat(128),
        };
        store
            .insert(unchecked(ev), &"wss://r/".into(), *ts)
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

    let results = store.query(&q, 100).unwrap();
    // Expect 5 events, newest-first: 3000, 2500, 2000, 1500, 1000
    assert_eq!(results.len(), 5, "must return all 5 events");
    let timestamps: Vec<u64> = results.iter().map(|e| e.raw.created_at).collect();
    assert_eq!(
        timestamps,
        vec![3000, 2500, 2000, 1500, 1000],
        "must be newest-first across authors"
    );
}

/// Limit is honoured.
#[test]
fn authors_kind_limit_respected() {
    let store = MemEventStore::new();
    let pk_a = make_pk(0x01);
    let pk_b = make_pk(0x02);

    for i in 0..10u64 {
        let ev = RawEvent {
            id: format!("01{i:062x}"),
            pubkey: pk_hex(0x01),
            created_at: 1000 + i,
            kind: 1,
            tags: vec![],
            content: String::new(),
            sig: "a".repeat(128),
        };
        store
            .insert(unchecked(ev), &"wss://r/".into(), 1000 + i)
            .unwrap();
        let ev2 = RawEvent {
            id: format!("02{i:062x}"),
            pubkey: pk_hex(0x02),
            created_at: 2000 + i,
            kind: 1,
            tags: vec![],
            content: String::new(),
            sig: "b".repeat(128),
        };
        store
            .insert(unchecked(ev2), &"wss://r/".into(), 2000 + i)
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
    // All 5 should be from author B (newest)
    for ev in &results {
        assert_eq!(ev.raw.pubkey, pk_hex(0x02), "top 5 must be from author B");
    }
}

/// since/until bounds are honoured.
#[test]
fn authors_kind_since_until_bounds() {
    let store = MemEventStore::new();
    let pk_a = make_pk(0x10);
    let pk_b = make_pk(0x20);

    for ts in [100u64, 200, 300, 400, 500] {
        let ev_a = RawEvent {
            id: format!("10{ts:062x}"),
            pubkey: pk_hex(0x10),
            created_at: ts,
            kind: 1,
            tags: vec![],
            content: String::new(),
            sig: "a".repeat(128),
        };
        store
            .insert(unchecked(ev_a), &"wss://r/".into(), ts)
            .unwrap();
        let ev_b = RawEvent {
            id: format!("20{ts:062x}"),
            pubkey: pk_hex(0x20),
            created_at: ts + 50,
            kind: 1,
            tags: vec![],
            content: String::new(),
            sig: "b".repeat(128),
        };
        store
            .insert(unchecked(ev_b), &"wss://r/".into(), ts + 50)
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
    // Results must still be newest-first
    for w in results.windows(2) {
        assert!(
            w[0].raw.created_at >= w[1].raw.created_at,
            "must be newest-first"
        );
    }
}

/// Only events with the requested kinds are returned.
#[test]
fn authors_kind_filters_by_kind() {
    let store = MemEventStore::new();
    let pk = make_pk(0x30);

    for (i, kind) in [1u32, 3, 7].iter().enumerate() {
        let ev = RawEvent {
            id: format!("30{i:062x}"),
            pubkey: pk_hex(0x30),
            created_at: 1000 + i as u64,
            kind: *kind,
            tags: vec![],
            content: String::new(),
            sig: "a".repeat(128),
        };
        store
            .insert(unchecked(ev), &"wss://r/".into(), 1000 + i as u64)
            .unwrap();
    }

    let mut authors = BTreeSet::new();
    authors.insert(pk);

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

/// Empty authors set returns no events.
#[test]
fn authors_kind_empty_authors_returns_nothing() {
    let store = MemEventStore::new();
    let ev = RawEvent {
        id: "aa".repeat(32),
        pubkey: pk_hex(0x40),
        created_at: 1000,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };
    store
        .insert(unchecked(ev), &"wss://r/".into(), 1000)
        .unwrap();

    let q = StoreQuery::AuthorsKind {
        authors: BTreeSet::new(),
        kinds: vec![1],
        since: None,
        until: None,
    };
    let results = store.query(&q, 100).unwrap();
    assert!(results.is_empty(), "empty authors must return nothing");
}

/// Single-author `AuthorKind` with an empty kind set returns nothing — same
/// positive-selection contract as `AuthorsKind` (parity with the LMDB backend).
#[test]
fn author_kind_empty_kinds_returns_nothing() {
    let store = MemEventStore::new();
    let pk = make_pk(0x46);
    let ev = RawEvent {
        id: "46".repeat(32),
        pubkey: pk_hex(0x46),
        created_at: 1000,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };
    store
        .insert(unchecked(ev), &"wss://r/".into(), 1000)
        .unwrap();

    let q = StoreQuery::AuthorKind {
        author: pk,
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

/// Empty kinds set returns nothing — `AuthorsKind` is a positive selection,
/// never a wildcard (parity with the LMDB backend; see the empty-set contract
/// on `StoreQuery::AuthorsKind`).
#[test]
fn authors_kind_empty_kinds_returns_nothing() {
    let store = MemEventStore::new();
    let pk = make_pk(0x45);
    let ev = RawEvent {
        id: "45".repeat(32),
        pubkey: pk_hex(0x45),
        created_at: 1000,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };
    store
        .insert(unchecked(ev), &"wss://r/".into(), 1000)
        .unwrap();

    let mut authors = BTreeSet::new();
    authors.insert(pk);

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

/// Cross-author dedup: same event ID inserted under two relay URLs must appear once.
#[test]
fn authors_kind_no_duplicate_event_ids() {
    let store = MemEventStore::new();
    let pk = make_pk(0x50);

    // Insert same-id event twice (from two relays; store deduplicates by id).
    let ev = RawEvent {
        id: "50".repeat(32),
        pubkey: pk_hex(0x50),
        created_at: 1000,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };
    store
        .insert(unchecked(ev.clone()), &"wss://r1/".into(), 1000)
        .unwrap();
    store
        .insert(unchecked(ev), &"wss://r2/".into(), 1000)
        .unwrap();

    let mut authors = BTreeSet::new();
    authors.insert(pk);

    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![1],
        since: None,
        until: None,
    };
    let results = store.query(&q, 100).unwrap();
    assert_eq!(results.len(), 1, "duplicate event id must appear only once");
}
