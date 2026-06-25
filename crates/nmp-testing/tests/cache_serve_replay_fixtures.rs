//! Mem ≡ LMDB parity fixtures — epic #1523.
//!
//! Each fixture runs against both `MemEventStore` and (when `lmdb-backend` is
//! enabled) `LmdbEventStore` via the `for_each_backend!` macro. The fixtures
//! validate that cache-serve replay results are byte-identical across both
//! backends: same count, same newest-first ordering, same event shapes.
//!
//! Run (mem only):
//!   cargo test -p nmp-testing --test cache_serve_replay_fixtures
//! Run (both backends):
//!   cargo test -p nmp-testing --features lmdb-backend --test cache_serve_replay_fixtures

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use nmp_store::{EventStore, StoreQuery, StoredEvent};
use nmp_testing::store_harness::{hex_to_id, StoreHarness, ALICE_HEX, ALICE_PUBKEY, BOB_HEX};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn collect_visit(store: &dyn EventStore, q: &StoreQuery, limit: usize) -> Vec<StoredEvent> {
    let mut out = Vec::new();
    store
        .query_visit(q, limit, &mut |ev: &StoredEvent| {
            out.push(ev.clone());
            ControlFlow::Continue(())
        })
        .unwrap();
    out
}

fn assert_newest_first(events: &[StoredEvent], context: &str) {
    for w in events.windows(2) {
        let (a, b) = (&w[0].raw, &w[1].raw);
        assert!(
            a.created_at > b.created_at
                || (a.created_at == b.created_at && a.id <= b.id),
            "{context}: ordering violated at created_at {} then {}",
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

// ─── Fixture: replay_feed ─────────────────────────────────────────────────────

/// Global feed: 150 kind:1 events, KindTime query, limit=150.
/// Assert: count=150, newest-first ordering.
fn replay_feed_body(h: &mut StoreHarness) {
    for i in 0..150u64 {
        h.insert(ALICE_HEX, 1, 1000 + i, "relay-fixture");
    }
    let q = StoreQuery::KindTime {
        kinds: vec![1],
        since: None,
        until: None,
    };
    let results = collect_visit(&*h.store, &q, 150);
    assert_eq!(results.len(), 150, "replay_feed: expected 150 results");
    assert_newest_first(&results, "replay_feed");
    assert_eq!(
        results[0].raw.created_at, 1149,
        "replay_feed: newest event should be at created_at=1149"
    );
    h.assert_invariants();
}

nmp_testing::for_each_backend!(replay_feed, replay_feed_body);

// ─── Fixture: replay_author_kind ─────────────────────────────────────────────

/// Home timeline: 60 authors × 3 kind:1 events each; AuthorsKind, limit=500.
/// Assert: count=180, newest-first.
fn replay_author_kind_body(h: &mut StoreHarness) {
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
    let results = collect_visit(&*h.store, &q, 500);
    assert_eq!(results.len(), 180, "replay_author_kind: expected 180 results");
    assert_newest_first(&results, "replay_author_kind");
    h.assert_invariants();
}

nmp_testing::for_each_backend!(replay_author_kind, replay_author_kind_body);

// ─── Fixture: replay_thread ───────────────────────────────────────────────────

/// Thread replay: root event + 80 replies (e-tag); Etag, limit=200.
/// Assert: count=80, newest-first (root excluded, only tagged replies).
fn replay_thread_body(h: &mut StoreHarness) {
    let root_hex = "a".repeat(64);
    let root_ev = h.make_event_with_id(&root_hex, ALICE_HEX, 1, 500);
    h.insert_raw(root_ev, "relay-fixture", 500_000);
    for i in 0..80u64 {
        let reply = h.make_event_with_tags(
            BOB_HEX,
            1,
            1000 + i,
            vec![vec!["e".into(), root_hex.clone()]],
        );
        h.insert_raw(reply, "relay-fixture", (1000 + i) * 1000);
    }
    let q = tags_query('e', &root_hex, vec![1]);
    let results = collect_visit(&*h.store, &q, 200);
    assert_eq!(results.len(), 80, "replay_thread: expected 80 replies");
    assert_newest_first(&results, "replay_thread");
    h.assert_invariants();
}

nmp_testing::for_each_backend!(replay_thread, replay_thread_body);

// ─── Generic single-letter tag fixtures (Mem ≡ LMDB parity) ──────────────────

/// Build a `StoreQuery::Tags` from authors, kinds, and `(letter, values)` dims.
fn mk_tags_query(
    authors: &[&str],
    kinds: &[u32],
    dims: &[(char, &[&str])],
    since: Option<u64>,
    until: Option<u64>,
) -> StoreQuery {
    let authors: BTreeSet<[u8; 32]> = authors.iter().map(|h| hex_to_id(h)).collect();
    let mut tags = std::collections::BTreeMap::new();
    for (letter, values) in dims {
        tags.insert(
            nostr::SingleLetterTag::from_char(*letter).unwrap(),
            values.iter().map(|v| v.to_string()).collect::<BTreeSet<String>>(),
        );
    }
    StoreQuery::Tags {
        authors,
        kinds: kinds.to_vec(),
        tags,
        since,
        until,
    }
}

/// `#h="room"` + kinds [9,11]: both NIP-29 group kinds hydrate (#2088), the
/// off-room and off-kind noise does not.
fn replay_tag_h_room_body(h: &mut StoreHarness) {
    for (i, kind) in [(0u64, 9u32), (1, 11), (2, 9), (3, 11)] {
        let ev = h.make_event_with_tags(BOB_HEX, kind, 1000 + i,
            vec![vec!["h".into(), "room".into()]]);
        h.insert_raw(ev, "relay-fixture", (1000 + i) * 1000);
    }
    // noise: same kinds, different room; right room, wrong kind.
    let n1 = h.make_event_with_tags(BOB_HEX, 9, 1500, vec![vec!["h".into(), "elsewhere".into()]]);
    h.insert_raw(n1, "relay-fixture", 1_500_000);
    let n2 = h.make_event_with_tags(BOB_HEX, 1, 1600, vec![vec!["h".into(), "room".into()]]);
    h.insert_raw(n2, "relay-fixture", 1_600_000);

    let q = mk_tags_query(&[], &[9, 11], &[('h', &["room"])], None, None);
    let results = collect_visit(&*h.store, &q, 100);
    assert_eq!(results.len(), 4, "#h=room + kinds[9,11] → exactly the 4 room events");
    assert_newest_first(&results, "replay_tag_h_room");
}

nmp_testing::for_each_backend!(replay_tag_h_room, replay_tag_h_room_body);

/// Multi-value OR (`#t=["nostr","nmp"]`) returns events carrying either value.
fn replay_tag_multivalue_or_body(h: &mut StoreHarness) {
    for (i, t) in ["nostr", "nmp", "bitcoin", "nostr"].iter().enumerate() {
        let ev = h.make_event_with_tags(BOB_HEX, 1, 1000 + i as u64,
            vec![vec!["t".into(), (*t).into()]]);
        h.insert_raw(ev, "relay-fixture", (1000 + i as u64) * 1000);
    }
    let q = mk_tags_query(&[], &[1], &[('t', &["nostr", "nmp"])], None, None);
    let results = collect_visit(&*h.store, &q, 100);
    assert_eq!(results.len(), 3, "#t OR [nostr,nmp] → 3 (two nostr + one nmp), not bitcoin");
    assert_newest_first(&results, "replay_tag_multivalue_or");
}

nmp_testing::for_each_backend!(replay_tag_multivalue_or, replay_tag_multivalue_or_body);

/// Multi-tag AND (`#h="room"` AND `#p=<alice>`): an event carrying only one of
/// the two tags must NOT match.
fn replay_tag_multi_and_body(h: &mut StoreHarness) {
    // both tags → matches.
    let both = h.make_event_with_tags(BOB_HEX, 9, 2000,
        vec![vec!["h".into(), "room".into()], vec!["p".into(), ALICE_HEX.into()]]);
    h.insert_raw(both, "relay-fixture", 2_000_000);
    // only #h → no match.
    let only_h = h.make_event_with_tags(BOB_HEX, 9, 2100, vec![vec!["h".into(), "room".into()]]);
    h.insert_raw(only_h, "relay-fixture", 2_100_000);
    // only #p → no match.
    let only_p = h.make_event_with_tags(BOB_HEX, 9, 2200, vec![vec!["p".into(), ALICE_HEX.into()]]);
    h.insert_raw(only_p, "relay-fixture", 2_200_000);

    let q = mk_tags_query(&[], &[9], &[('h', &["room"]), ('p', &[ALICE_HEX])], None, None);
    let results = collect_visit(&*h.store, &q, 100);
    assert_eq!(results.len(), 1, "multi-tag AND must exclude one-tag events");
    h.assert_invariants();
}

nmp_testing::for_each_backend!(replay_tag_multi_and, replay_tag_multi_and_body);

/// Tag-only query with empty kinds is an any-kind wildcard.
fn replay_tag_only_no_kinds_body(h: &mut StoreHarness) {
    for (i, kind) in [(0u64, 1u32), (1, 9), (2, 30023)] {
        let ev = h.make_event_with_tags(BOB_HEX, kind, 1000 + i,
            vec![vec!["t".into(), "nostr".into()]]);
        h.insert_raw(ev, "relay-fixture", (1000 + i) * 1000);
    }
    let q = mk_tags_query(&[], &[], &[('t', &["nostr"])], None, None);
    let results = collect_visit(&*h.store, &q, 100);
    assert_eq!(results.len(), 3, "empty kinds = any kind: all 3 #t=nostr events match");
}

nmp_testing::for_each_backend!(replay_tag_only_no_kinds, replay_tag_only_no_kinds_body);

/// Author + tag + kind: the author scope filters to one of two posters.
fn replay_tag_author_kind_body(h: &mut StoreHarness) {
    let a = h.make_event_with_tags(ALICE_HEX, 9, 3000, vec![vec!["h".into(), "room".into()]]);
    h.insert_raw(a, "relay-fixture", 3_000_000);
    let b = h.make_event_with_tags(BOB_HEX, 9, 3100, vec![vec!["h".into(), "room".into()]]);
    h.insert_raw(b, "relay-fixture", 3_100_000);

    let q = mk_tags_query(&[ALICE_HEX], &[9], &[('h', &["room"])], None, None);
    let results = collect_visit(&*h.store, &q, 100);
    assert_eq!(results.len(), 1, "author-scoped tag query → only ALICE's event");
    assert_eq!(results[0].raw.pubkey, ALICE_HEX);
}

nmp_testing::for_each_backend!(replay_tag_author_kind, replay_tag_author_kind_body);

/// `since`/`until` bound a tag scan inclusively.
fn replay_tag_since_until_body(h: &mut StoreHarness) {
    for i in 0..10u64 {
        let ev = h.make_event_with_tags(BOB_HEX, 1, 1000 + i,
            vec![vec!["t".into(), "nostr".into()]]);
        h.insert_raw(ev, "relay-fixture", (1000 + i) * 1000);
    }
    // window [1003, 1006] inclusive → 4 events.
    let q = mk_tags_query(&[], &[1], &[('t', &["nostr"])], Some(1003), Some(1006));
    let results = collect_visit(&*h.store, &q, 100);
    assert_eq!(results.len(), 4, "since/until inclusive window → 4 events");
    for ev in &results {
        assert!((1003..=1006).contains(&ev.raw.created_at));
    }
}

nmp_testing::for_each_backend!(replay_tag_since_until, replay_tag_since_until_body);

/// Query-visit early break for `Tags`: stop after N visited.
fn replay_tag_visit_break_body(h: &mut StoreHarness) {
    for i in 0..20u64 {
        let ev = h.make_event_with_tags(BOB_HEX, 1, 1000 + i,
            vec![vec!["t".into(), "nostr".into()]]);
        h.insert_raw(ev, "relay-fixture", (1000 + i) * 1000);
    }
    let q = mk_tags_query(&[], &[1], &[('t', &["nostr"])], None, None);
    let mut seen = 0usize;
    h.store
        .query_visit(&q, 100, &mut |_: &StoredEvent| {
            seen += 1;
            if seen >= 5 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert_eq!(seen, 5, "early Break stops the Tags visit at 5");
}

nmp_testing::for_each_backend!(replay_tag_visit_break, replay_tag_visit_break_body);

// ─── Fixture: replay_dm_ciphertext ───────────────────────────────────────────

/// DM ciphertext replay: ALICE sends 40 kind:4 + 40 kind:14 events.
/// AuthorKind, limit=200. Assert: count=80, no kind:1 noise.
fn replay_dm_ciphertext_body(h: &mut StoreHarness) {
    for i in 0..40u64 {
        h.insert(ALICE_HEX, 4, 1000 + i, "relay-fixture");
    }
    for i in 0..40u64 {
        h.insert(ALICE_HEX, 14, 2000 + i, "relay-fixture");
    }
    // Noise: 20 kind:1 events that must not appear in DM results
    for i in 0..20u64 {
        h.insert(ALICE_HEX, 1, 3000 + i, "relay-fixture");
    }
    let q = StoreQuery::AuthorKind {
        author: ALICE_PUBKEY,
        kinds: vec![4, 14],
        since: None,
        until: None,
    };
    let results = collect_visit(&*h.store, &q, 200);
    assert_eq!(results.len(), 80, "replay_dm_ciphertext: expected 80 DM events");
    for ev in &results {
        assert!(
            ev.raw.kind == 4 || ev.raw.kind == 14,
            "replay_dm_ciphertext: unexpected kind {} in DM results",
            ev.raw.kind
        );
    }
    assert_newest_first(&results, "replay_dm_ciphertext");
    h.assert_invariants();
}

nmp_testing::for_each_backend!(replay_dm_ciphertext, replay_dm_ciphertext_body);

// ─── Fixture: replay_profile_metadata ────────────────────────────────────────

/// Profile metadata: 5 kind:0 events from ALICE (replaceable).
/// AuthorKind, assert count=1 (only newest survives replaceable semantics).
fn replay_profile_metadata_body(h: &mut StoreHarness) {
    for i in 0..5u64 {
        h.insert(ALICE_HEX, 0, 1000 + i, "relay-fixture");
    }
    // Noise: kind:1 events for the same author
    for i in 0..10u64 {
        h.insert(ALICE_HEX, 1, 2000 + i, "relay-fixture");
    }
    let q = StoreQuery::AuthorKind {
        author: ALICE_PUBKEY,
        kinds: vec![0],
        since: None,
        until: None,
    };
    let results = collect_visit(&*h.store, &q, 50);
    // kind:0 is replaceable — only 1 event survives (the newest)
    assert_eq!(
        results.len(),
        1,
        "replay_profile_metadata: kind:0 replaceable should leave only 1 event; got {}",
        results.len()
    );
    assert_eq!(
        results[0].raw.created_at, 1004,
        "replay_profile_metadata: newest kind:0 should have created_at=1004"
    );
    h.assert_invariants();
}

nmp_testing::for_each_backend!(replay_profile_metadata, replay_profile_metadata_body);

// ─── Fixture: replay_relay_provenance (skipped) ──────────────────────────────

// TODO: relay_provenance fixture requires a StoreQuery variant that correlates
// relay-origin metadata with event content — this is not yet in the
// `StoreQuery` enum. Filed as part of epic #1523 follow-up work.
// When the variant lands, add a `replay_relay_provenance` fixture here using
// a `#p`-tag `StoreQuery::Tags` over ALICE for kinds [3, 10002] as a
// stand-in for the relay-provenance discovery pattern.
