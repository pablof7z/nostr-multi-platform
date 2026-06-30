//! Materialization regression gate — epic #1523.
//!
//! Every test asserts that the LMDB `query_visit` streaming path converts
//! **at most** as many events from LMDB format as the visitor actually
//! consumes.  An early `ControlFlow::Break` must stop conversion immediately;
//! no trailing over-scan is allowed.
//!
//! Run: cargo test -p nmp-testing --features lmdb-backend \
//!        --test cache_no_materialization_gate

#![cfg(feature = "lmdb-backend")]

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use nmp_store::{conversion_count, reset_conversion_count, EventStore, StoreQuery, StoredEvent};
use nmp_testing::store_harness::{hex_to_id, StoreHarness, ALICE_HEX, ALICE_PUBKEY, BOB_HEX};

// ─── helpers ─────────────────────────────────────────────────────────────────

// CONVERSION_COUNT is a process-wide static; tests run in parallel and would
// race on reset→read.  Serialise every counter-sensitive section with this
// mutex so each test sees only its own conversions.
static COUNTER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn author_hex(i: u8) -> String {
    format!("{:02x}{}", i, "0".repeat(62))
}

fn author_pubkey(i: u8) -> [u8; 32] {
    hex_to_id(&author_hex(i))
}

/// Build a single-letter `StoreQuery::Tags` (one tag dimension, one value).
fn tags_query(letter: char, value: &str, kinds: Vec<u32>) -> StoreQuery {
    let mut tags = std::collections::BTreeMap::new();
    tags.insert(
        nostr::SingleLetterTag::from_char(letter).unwrap(),
        BTreeSet::from([value.to_string()]),
    );
    StoreQuery::Tags {
        authors: BTreeSet::new(),
        kinds,
        tags,
        since: None,
        until: None,
    }
}

/// Visit `limit` events from `q` breaking after `break_after` have been seen.
/// Returns `(visited, converted)` where `converted` is the global LMDB
/// materialization count after the call.
fn visit_break_after(
    store: &dyn EventStore,
    q: &StoreQuery,
    limit: usize,
    break_after: usize,
) -> (usize, usize) {
    let _guard = COUNTER_LOCK.lock().unwrap();
    reset_conversion_count();
    let mut visited = 0usize;
    store
        .query_visit(q, limit, &mut |_: &StoredEvent| {
            visited += 1;
            if visited >= break_after {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    let converted = conversion_count();
    (visited, converted)
}

// ─── Deliverable 2 tests ─────────────────────────────────────────────────────

/// Insert 100 kind:1 events; visit with limit=1000 but break after 10.
/// The LMDB path must convert exactly 10 events — not the full 100.
#[test]
fn early_break_converts_exactly_n_not_full_corpus() {
    let h = StoreHarness::lmdb();
    for i in 0..100u64 {
        h.insert(ALICE_HEX, 1, 1000 + i, "relay-fixture");
    }
    let q = StoreQuery::KindTime {
        kinds: vec![1],
        since: None,
        until: None,
    };
    let (visited, converted) = visit_break_after(&*h.store, &q, 1000, 10);
    assert_eq!(visited, 10, "visitor must break after 10");
    assert_eq!(
        converted, 10,
        "must convert exactly 10 events (no over-scan)"
    );
    h.assert_invariants();
}

/// Insert events; visit with limit=25 (continue-only).
/// Conversion count must not exceed 25.
#[test]
fn limit_caps_conversions_no_over_scan() {
    let h = StoreHarness::lmdb();
    for i in 0..100u64 {
        h.insert(ALICE_HEX, 1, 1000 + i, "relay-fixture");
    }
    let q = StoreQuery::KindTime {
        kinds: vec![1],
        since: None,
        until: None,
    };
    let _guard = COUNTER_LOCK.lock().unwrap();
    reset_conversion_count();
    let mut visited = 0usize;
    h.store
        .query_visit(&q, 25, &mut |_: &StoredEvent| {
            visited += 1;
            ControlFlow::Continue(())
        })
        .unwrap();
    let converted = conversion_count();
    assert!(
        converted <= 25,
        "converted {converted} events but limit was 25"
    );
    assert_eq!(visited, 25, "should visit all 25 in the limit window");
    h.assert_invariants();
}

/// KindTime streaming: 60 kind:1 events; break after 5.
/// Asserts exactly 5 conversions (streaming stops at Break).
#[test]
fn kindtime_streaming() {
    let h = StoreHarness::lmdb();
    for i in 0..60u64 {
        h.insert(ALICE_HEX, 1, 1000 + i, "relay-fixture");
    }
    let q = StoreQuery::KindTime {
        kinds: vec![1],
        since: None,
        until: None,
    };
    let (visited, converted) = visit_break_after(&*h.store, &q, 1000, 5);
    assert_eq!(visited, 5);
    assert_eq!(converted, 5, "KindTime: break at 5 must convert exactly 5");
    h.assert_invariants();
}

/// AuthorKind streaming: 60 kind:1 events from ALICE; break after 5.
#[test]
fn authorkind_streaming() {
    let h = StoreHarness::lmdb();
    for i in 0..60u64 {
        h.insert(ALICE_HEX, 1, 1000 + i, "relay-fixture");
    }
    let q = StoreQuery::AuthorKind {
        author: ALICE_PUBKEY,
        kinds: vec![1],
        since: None,
        until: None,
    };
    let (visited, converted) = visit_break_after(&*h.store, &q, 1000, 5);
    assert_eq!(visited, 5);
    assert_eq!(
        converted, 5,
        "AuthorKind: break at 5 must convert exactly 5"
    );
    h.assert_invariants();
}

/// AuthorsKind streaming: 3 authors × 20 events each; break after 5.
#[test]
fn authorskind_streaming() {
    let h = StoreHarness::lmdb();
    let mut authors = BTreeSet::new();
    for a in 1u8..=3 {
        let ahex = author_hex(a);
        authors.insert(author_pubkey(a));
        for i in 0..20u64 {
            h.insert(&ahex, 1, (a as u64) * 100 + i, "relay-fixture");
        }
    }
    let q = StoreQuery::AuthorsKind {
        authors,
        kinds: vec![1],
        since: None,
        until: None,
    };
    let (visited, converted) = visit_break_after(&*h.store, &q, 1000, 5);
    assert_eq!(visited, 5);
    assert_eq!(
        converted, 5,
        "AuthorsKind: break at 5 must convert exactly 5"
    );
    h.assert_invariants();
}

/// KindDtag streaming: 60 kind:30023 events with d-tag "test-gate"; break after 5.
#[test]
fn kinddtag_streaming() {
    let h = StoreHarness::lmdb();
    for i in 1u8..=30 {
        // different authors so param-replaceable keying doesn't collapse them
        let ahex = author_hex(i);
        let ev = h.make_event_with_tags(
            &ahex,
            30023,
            1000 + i as u64,
            vec![vec!["d".into(), "test-gate".into()]],
        );
        h.insert_raw(ev, "relay-fixture", (1000 + i as u64) * 1000);
    }
    for i in 31u8..=60 {
        let ahex = author_hex(i);
        let ev = h.make_event_with_tags(
            &ahex,
            30023,
            2000 + i as u64,
            vec![vec!["d".into(), "test-gate".into()]],
        );
        h.insert_raw(ev, "relay-fixture", (2000 + i as u64) * 1000);
    }
    let q = StoreQuery::KindDtag {
        kind: 30023,
        d_tag: b"test-gate".to_vec(),
        since: None,
        until: None,
    };
    let (visited, converted) = visit_break_after(&*h.store, &q, 1000, 5);
    assert_eq!(visited, 5);
    assert_eq!(converted, 5, "KindDtag: break at 5 must convert exactly 5");
    h.assert_invariants();
}

/// Etag streaming: root event + 60 replies (e-tag); break after 5.
#[test]
fn etag_streaming() {
    let h = StoreHarness::lmdb();
    let root_hex = "a".repeat(64);
    let root_ev = h.make_event_with_id(&root_hex, ALICE_HEX, 1, 500);
    h.insert_raw(root_ev, "relay-fixture", 500_000);
    for i in 0..60u64 {
        let reply = h.make_event_with_tags(
            BOB_HEX,
            1,
            1000 + i,
            vec![vec!["e".into(), root_hex.clone()]],
        );
        h.insert_raw(reply, "relay-fixture", (1000 + i) * 1000);
    }
    let q = tags_query('e', &root_hex, vec![1]);
    let (visited, converted) = visit_break_after(&*h.store, &q, 1000, 5);
    assert_eq!(visited, 5);
    assert_eq!(converted, 5, "Etag: break at 5 must convert exactly 5");
    h.assert_invariants();
}

/// Ptag streaming: 60 events p-tagging ALICE; break after 5.
#[test]
fn ptag_streaming() {
    let h = StoreHarness::lmdb();
    for i in 0..60u64 {
        let ev = h.make_event_with_tags(
            BOB_HEX,
            1,
            1000 + i,
            vec![vec!["p".into(), ALICE_HEX.into()]],
        );
        h.insert_raw(ev, "relay-fixture", (1000 + i) * 1000);
    }
    let q = tags_query('p', ALICE_HEX, vec![1]);
    let (visited, converted) = visit_break_after(&*h.store, &q, 1000, 5);
    assert_eq!(visited, 5);
    assert_eq!(converted, 5, "Ptag: break at 5 must convert exactly 5");
    h.assert_invariants();
}
