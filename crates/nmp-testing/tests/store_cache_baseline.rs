//! Baseline fixtures and regression tests for all six `StoreQuery` variants
//! against `LmdbEventStore`. Captured before PR #1516 (true streaming query_visit).
//!
//! Run: cargo test -p nmp-testing --features lmdb-backend --test store_cache_baseline

#![cfg(feature = "lmdb-backend")]

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use nmp_store::{EventStore, StoreQuery, StoredEvent};
use nmp_testing::store_harness::{hex_to_id, StoreHarness, ALICE_HEX, ALICE_PUBKEY, BOB_HEX};

// ─── helpers ─────────────────────────────────────────────────────────────────

fn count_visited(store: &dyn EventStore, q: &StoreQuery, limit: usize) -> usize {
    let mut n = 0usize;
    store
        .query_visit(q, limit, &mut |_: &StoredEvent| {
            n += 1;
            ControlFlow::Continue(())
        })
        .unwrap();
    n
}

fn assert_newest_first(events: &[StoredEvent]) {
    for w in events.windows(2) {
        let (a, b) = (&w[0].raw, &w[1].raw);
        assert!(
            a.created_at > b.created_at
                || (a.created_at == b.created_at && a.id <= b.id),
            "ordering violated: {} then {}",
            a.created_at,
            b.created_at
        );
    }
}

fn author_hex(i: u8) -> String {
    format!("{:02x}{}", i, "0".repeat(62))
}

fn author_pubkey(i: u8) -> [u8; 32] {
    hex_to_id(&author_hex(i))
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// Global feed: KindTime over kinds [1, 6, 16] — 150 events, newest-first.
#[test]
fn feed_kindtime_global_notes() {
    let h = StoreHarness::lmdb();
    let kinds = [1u32, 6, 16];
    for i in 0..150u64 {
        let kind = kinds[(i as usize) % 3];
        h.insert(ALICE_HEX, kind, 1000 + i, "relay-fixture");
    }
    let q = StoreQuery::KindTime {
        kinds: vec![1, 6, 16],
        since: None,
        until: None,
    };
    let results = h.store.query(&q, 200).unwrap();
    assert_eq!(results.len(), 150, "expected 150 results");
    assert_newest_first(&results);
    assert_eq!(count_visited(&*h.store, &q, 200), 150);
    assert_eq!(results[0].raw.created_at, 1149);
    h.assert_invariants();
}

/// KindTime with limit — only kind:1 events, limit=20.
#[test]
fn feed_kindtime_limit_honored() {
    let h = StoreHarness::lmdb();
    let kinds = [1u32, 6, 16];
    for i in 0..150u64 {
        let kind = kinds[(i as usize) % 3];
        h.insert(ALICE_HEX, kind, 1000 + i, "relay-fixture");
    }
    // kind:1 events are at created_at 1000,1003,1006,...1147 (50 of them)
    // highest kind:1 created_at is 1147
    let q = StoreQuery::KindTime {
        kinds: vec![1],
        since: None,
        until: None,
    };
    let results = h.store.query(&q, 20).unwrap();
    assert_eq!(results.len(), 20);
    assert_newest_first(&results);
    assert_eq!(count_visited(&*h.store, &q, 20), 20);
    assert_eq!(results[0].raw.created_at, 1147);
    assert_eq!(results[0].raw.kind, 1);
    h.assert_invariants();
}

/// Home timeline: 60 authors x 3 kind:1 events = 180 events via AuthorsKind.
#[test]
fn home_timeline_authorskind() {
    let h = StoreHarness::lmdb();
    let mut authors: BTreeSet<[u8; 32]> = BTreeSet::new();
    for i in 1u8..=60 {
        let ahex = author_hex(i);
        authors.insert(author_pubkey(i));
        let base = i as u64 * 3;
        h.insert(&ahex, 1, base, "relay-fixture");
        h.insert(&ahex, 1, base + 1, "relay-fixture");
        h.insert(&ahex, 1, base + 2, "relay-fixture");
    }
    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![1],
        since: None,
        until: None,
    };
    let results = h.store.query(&q, 500).unwrap();
    assert_eq!(results.len(), 180);
    assert_newest_first(&results);
    assert_eq!(count_visited(&*h.store, &q, 500), 180);
    h.assert_invariants();
}

/// AuthorsKind with since filter — only events >= created_at 90.
#[test]
fn home_timeline_authorskind_since_filter() {
    let h = StoreHarness::lmdb();
    let mut authors: BTreeSet<[u8; 32]> = BTreeSet::new();
    for i in 1u8..=60 {
        let ahex = author_hex(i);
        authors.insert(author_pubkey(i));
        let base = i as u64 * 3;
        h.insert(&ahex, 1, base, "relay-fixture");
        h.insert(&ahex, 1, base + 1, "relay-fixture");
        h.insert(&ahex, 1, base + 2, "relay-fixture");
    }
    // Authors 30..=60 have base >= 90: 31 authors x 3 = 93 events
    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![1],
        since: Some(90),
        until: None,
    };
    let results = h.store.query(&q, 500).unwrap();
    assert_eq!(results.len(), 93);
    assert_newest_first(&results);
    h.assert_invariants();
}

/// Thread replay: Etag query for 80 replies to a root event.
#[test]
fn thread_etag() {
    let h = StoreHarness::lmdb();
    let root_hex = "a".repeat(64);
    let root_ev = h.make_event_with_id(&root_hex, ALICE_HEX, 1, 500);
    h.insert_raw(root_ev, "relay-fixture", 500_000);
    let root_id = hex_to_id(&root_hex);
    for i in 0..80u64 {
        let reply = h.make_event_with_tags(
            BOB_HEX,
            1,
            600 + i,
            vec![vec!["e".into(), root_hex.clone()]],
        );
        h.insert_raw(reply, "relay-fixture", (600 + i) * 1000);
    }
    // 40 unrelated events
    for i in 0..40u64 {
        h.insert(ALICE_HEX, 1, 1000 + i, "relay-fixture");
    }
    let q = StoreQuery::Etag {
        target: root_id,
        kinds: vec![1],
    };
    let results = h.store.query(&q, 200).unwrap();
    assert_eq!(results.len(), 80);
    assert_newest_first(&results);
    assert_eq!(count_visited(&*h.store, &q, 200), 80);
    h.assert_invariants();
}

/// Mention inbox: Ptag query for 70 events p-tagging ALICE.
#[test]
fn mentions_ptag() {
    let h = StoreHarness::lmdb();
    for i in 0..70u64 {
        let ev = h.make_event_with_tags(
            BOB_HEX,
            1,
            1000 + i,
            vec![vec!["p".into(), ALICE_HEX.into()]],
        );
        h.insert_raw(ev, "relay-fixture", (1000 + i) * 1000);
    }
    for i in 0..30u64 {
        let ev = h.make_event_with_tags(
            ALICE_HEX,
            1,
            2000 + i,
            vec![vec!["p".into(), BOB_HEX.into()]],
        );
        h.insert_raw(ev, "relay-fixture", (2000 + i) * 1000);
    }
    let q = StoreQuery::Ptag {
        target: hex_to_id(ALICE_HEX),
        kinds: vec![1],
    };
    let results = h.store.query(&q, 200).unwrap();
    assert_eq!(results.len(), 70);
    assert_newest_first(&results);
    assert_eq!(count_visited(&*h.store, &q, 200), 70);
    h.assert_invariants();
}

/// DM inbox: AuthorKind for kinds [4, 14] from ALICE -- 80 DM events, no kind:1 noise.
#[test]
fn dm_inbox_authorkind_replay() {
    let h = StoreHarness::lmdb();
    for i in 0..40u64 {
        h.insert(ALICE_HEX, 4, 1000 + i, "relay-fixture");
    }
    for i in 0..40u64 {
        h.insert(ALICE_HEX, 14, 2000 + i, "relay-fixture");
    }
    for i in 0..20u64 {
        h.insert(ALICE_HEX, 1, 3000 + i, "relay-fixture");
    }
    let q = StoreQuery::AuthorKind {
        author: ALICE_PUBKEY,
        kinds: vec![4, 14],
        since: None,
        until: None,
    };
    let results = h.store.query(&q, 200).unwrap();
    assert_eq!(results.len(), 80);
    for ev in &results {
        assert!(
            ev.raw.kind == 4 || ev.raw.kind == 14,
            "unexpected kind {} in DM results",
            ev.raw.kind
        );
    }
    assert_newest_first(&results);
    assert_eq!(count_visited(&*h.store, &q, 200), 80);
    h.assert_invariants();
}

/// Profile metadata: kind:0 is replaceable -- only the newest survives.
#[test]
fn profile_metadata_kind0() {
    let h = StoreHarness::lmdb();
    for i in 0..5u64 {
        h.insert(ALICE_HEX, 0, 1000 + i, "relay-fixture");
    }
    for i in 0..10u64 {
        h.insert(ALICE_HEX, 1, 2000 + i, "relay-fixture");
    }
    let q = StoreQuery::AuthorKind {
        author: ALICE_PUBKEY,
        kinds: vec![0],
        since: None,
        until: None,
    };
    let results = h.store.query(&q, 50).unwrap();
    // kind:0 is replaceable -- only 1 event survives (the newest)
    assert_eq!(results.len(), 1, "kind:0 replaceable: only 1 should survive");
    assert_eq!(results[0].raw.created_at, 1004);
    h.assert_invariants();
}

/// Relay provenance candidates: Ptag for kinds [3, 10002] from distinct authors.
#[test]
fn relay_provenance_candidates_ptag() {
    let h = StoreHarness::lmdb();
    // 25 authors posting kind:3 with p-tag ALICE
    for i in 1u8..=25 {
        let ahex = author_hex(i);
        let ev = h.make_event_with_tags(
            &ahex,
            3,
            1000 + i as u64,
            vec![vec!["p".into(), ALICE_HEX.into()]],
        );
        h.insert_raw(ev, "relay-fixture", (1000 + i as u64) * 1000);
    }
    // 25 authors posting kind:10002 with p-tag ALICE
    for i in 26u8..=50 {
        let ahex = author_hex(i);
        let ev = h.make_event_with_tags(
            &ahex,
            10002,
            2000 + i as u64,
            vec![vec!["p".into(), ALICE_HEX.into()]],
        );
        h.insert_raw(ev, "relay-fixture", (2000 + i as u64) * 1000);
    }
    // 10 authors posting kind:1 with p-tag ALICE (noise)
    for i in 51u8..=60 {
        let ahex = author_hex(i);
        let ev = h.make_event_with_tags(
            &ahex,
            1,
            3000 + i as u64,
            vec![vec!["p".into(), ALICE_HEX.into()]],
        );
        h.insert_raw(ev, "relay-fixture", (3000 + i as u64) * 1000);
    }
    let q = StoreQuery::Ptag {
        target: ALICE_PUBKEY,
        kinds: vec![3, 10002],
    };
    let results = h.store.query(&q, 100).unwrap();
    assert_eq!(results.len(), 50);
    for ev in &results {
        assert!(
            ev.raw.kind == 3 || ev.raw.kind == 10002,
            "unexpected kind {} in relay provenance results",
            ev.raw.kind
        );
    }
    assert_newest_first(&results);
    assert_eq!(count_visited(&*h.store, &q, 100), 50);
    h.assert_invariants();
}

/// Parameterized replaceable: KindDtag for kind:30023 with d-tag "slug-a".
#[test]
fn kinddtag_param_replaceable() {
    let h = StoreHarness::lmdb();
    // 8 distinct authors posting kind:30023 with d-tag "slug-a"
    for i in 1u8..=8 {
        let ahex = author_hex(i);
        let ev = h.make_event_with_tags(
            &ahex,
            30023,
            1000 + i as u64,
            vec![vec!["d".into(), "slug-a".into()]],
        );
        h.insert_raw(ev, "relay-fixture", (1000 + i as u64) * 1000);
    }
    // 4 distinct authors posting kind:30023 with d-tag "slug-b"
    for i in 9u8..=12 {
        let ahex = author_hex(i);
        let ev = h.make_event_with_tags(
            &ahex,
            30023,
            2000 + i as u64,
            vec![vec!["d".into(), "slug-b".into()]],
        );
        h.insert_raw(ev, "relay-fixture", (2000 + i as u64) * 1000);
    }
    let q = StoreQuery::KindDtag {
        kind: 30023,
        d_tag: b"slug-a".to_vec(),
        since: None,
        until: None,
    };
    let results = h.store.query(&q, 50).unwrap();
    assert_eq!(results.len(), 8);
    assert_newest_first(&results);
    assert_eq!(count_visited(&*h.store, &q, 50), 8);
    h.assert_invariants();
}

/// Early break: query_visit stops exactly when visitor returns Break.
#[test]
fn early_break_stops_visit() {
    let h = StoreHarness::lmdb();
    for i in 0..150u64 {
        h.insert(ALICE_HEX, 1, 1000 + i, "relay-fixture");
    }
    let q = StoreQuery::KindTime {
        kinds: vec![1],
        since: None,
        until: None,
    };
    let mut visited = 0usize;
    h.store
        .query_visit(&q, 1000, &mut |_: &StoredEvent| {
            visited += 1;
            if visited >= 10 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert_eq!(visited, 10, "visitor must stop after exactly 10 events");
    h.assert_invariants();
}
