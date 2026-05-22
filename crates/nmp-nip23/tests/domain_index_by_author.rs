#![cfg(feature = "long-form")]
//! Domain-store integration: 3 articles by alice + 1 by bob → `by_author`
//! reverse index returns 3 for alice, 4 total via the unfiltered list. Proves
//! that `decode_and_route` writes the indexes the views read at query time.

mod common;

use common::article;
use nmp_core::store::{EventStore, MemEventStore};
use nmp_nip23::{decode_and_route, get, list_all, list_by_author, NAMESPACE};

const ALICE: &str = "alice-pubkey-0000000000000000000000000000000000000000000000000000000";
const BOB: &str = "bob-pubkey-000000000000000000000000000000000000000000000000000000000000";

#[test]
fn by_author_returns_three_for_alice_and_four_for_unfiltered() {
    // The author strings here are placeholders — they're written as opaque
    // keys into the domain store, no signature verification is happening.
    let store = MemEventStore::new();
    let handle = store
        .domain_open(NAMESPACE)
        .expect("namespace opens for first time");

    // Alice publishes 3 articles, bob 1. published_at chosen so the sort
    // order is alice2 > bob > alice1 > alice3 (when listed by `published_at`
    // desc).
    let alice1 = article(&"a".repeat(64), ALICE, 100, "alice-1", Some("Alice One"), Some(100), "body");
    let alice2 = article(&"b".repeat(64), ALICE, 200, "alice-2", Some("Alice Two"), Some(300), "body");
    let alice3 = article(&"c".repeat(64), ALICE, 300, "alice-3", Some("Alice Three"), Some(50), "body");
    let bob1 = article(&"d".repeat(64), BOB, 400, "bob-1", Some("Bob One"), Some(200), "body");

    for evt in [&alice1, &alice2, &alice3, &bob1] {
        decode_and_route(evt, &handle).expect("decode_and_route is infallible for valid kinds");
    }

    let alice_records = list_by_author(&handle, ALICE).expect("list_by_author succeeds");
    assert_eq!(alice_records.len(), 3, "alice published 3 articles");
    let alice_d_tags: Vec<&str> = alice_records.iter().map(|r| r.d_tag.as_str()).collect();
    assert!(alice_d_tags.contains(&"alice-1"));
    assert!(alice_d_tags.contains(&"alice-2"));
    assert!(alice_d_tags.contains(&"alice-3"));

    let all = list_all(&handle).expect("list_all succeeds");
    assert_eq!(all.len(), 4, "four articles total");
}

#[test]
fn list_by_author_returns_empty_when_author_absent() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();
    let records = list_by_author(&handle, "nobody").expect("scan on empty store works");
    assert!(records.is_empty());
}

#[test]
fn list_all_returns_articles_sorted_by_published_at_desc() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let oldest = article(&"a".repeat(64), ALICE, 0, "old", None, Some(100), "");
    let middle = article(&"b".repeat(64), ALICE, 0, "mid", None, Some(200), "");
    let newest = article(&"c".repeat(64), ALICE, 0, "new", None, Some(300), "");

    decode_and_route(&middle, &handle).unwrap();
    decode_and_route(&oldest, &handle).unwrap();
    decode_and_route(&newest, &handle).unwrap();

    let listed = list_all(&handle).unwrap();
    let order: Vec<&str> = listed.iter().map(|r| r.d_tag.as_str()).collect();
    assert_eq!(order, vec!["new", "mid", "old"]);
}

#[test]
fn stale_redelivery_does_not_overwrite_a_newer_record() {
    // D4 / NIP-33 replaceable correctness: a relay can redeliver an older
    // revision of the same `(author, d_tag)` after the newer one already
    // landed (reconnect backfill, multi-relay fan-in). The stale event must
    // NOT clobber the current record. Insert the newer revision first, then
    // redeliver the older one, then assert the newer survived.
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let newer = article(
        &"b".repeat(64),
        ALICE,
        200,
        "intro",
        Some("Revised Title"),
        None,
        "revised body",
    );
    let older = article(
        &"a".repeat(64),
        ALICE,
        100,
        "intro",
        Some("Original Title"),
        None,
        "original body",
    );

    decode_and_route(&newer, &handle).unwrap();
    // Redeliver the staler event after the newer one already landed.
    decode_and_route(&older, &handle).unwrap();

    let record = get(&handle, ALICE, "intro")
        .expect("get succeeds")
        .expect("a record is present");
    assert_eq!(record.created_at, 200, "the newer revision must survive");
    assert_eq!(record.event_id, "b".repeat(64));
    assert_eq!(record.title.as_deref(), Some("Revised Title"));
    assert_eq!(record.content, "revised body");

    // And the in-order case still works: a genuinely newer revision wins.
    let newest = article(&"c".repeat(64), ALICE, 300, "intro", Some("Newest"), None, "newest body");
    decode_and_route(&newest, &handle).unwrap();
    let after = get(&handle, ALICE, "intro").unwrap().unwrap();
    assert_eq!(after.created_at, 300, "a newer revision still replaces");
    assert_eq!(after.event_id, "c".repeat(64));

    // Only one primary row for the pair throughout.
    let all = list_all(&handle).unwrap();
    assert_eq!(all.len(), 1, "one (author, d_tag) primary row");
}

#[test]
fn decode_and_route_is_a_noop_for_wrong_kind() {
    use common::stored;

    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let kind_one_event = stored(
        &"a".repeat(64),
        ALICE,
        1,
        0,
        vec![vec!["d".into(), "ignored".into()]],
        "body",
    );
    decode_and_route(&kind_one_event, &handle).unwrap();

    let listed = list_all(&handle).unwrap();
    assert!(listed.is_empty(), "kind:1 must not enter the articles store");
}
