//! Domain-store integration: `decode_and_route` writes the composite reverse
//! indexes; `list_by_author_kind` returns the right subset; stale-redelivery
//! must not overwrite a newer record for BOTH a replaceable (10000) and a
//! parameterized (30000) kind; replaceable-supersession keeps one primary row;
//! wrong-kind is a no-op.

mod common;

use common::{list_event, set_event, stored};
use nmp_core::store::{EventStore, MemEventStore};
use nmp_nip51::{
    decode_and_route, get, list_all, list_by_author, list_by_author_kind, KIND_FOLLOW_SETS,
    KIND_MUTE_LIST, NAMESPACE,
};

const ALICE: &str = "alice-pubkey-0000000000000000000000000000000000000000000000000000000";
const BOB: &str = "bob-pubkey-000000000000000000000000000000000000000000000000000000000000";

#[test]
fn list_by_author_kind_returns_the_right_subset() {
    // Alice: 1 mute list + 2 follow-sets. Bob: 1 mute list.
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).expect("namespace opens");

    let alice_mute = list_event(&"a".repeat(64), ALICE, KIND_MUTE_LIST, 100, vec![], "");
    let alice_fs1 = set_event(
        &"b".repeat(64),
        ALICE,
        KIND_FOLLOW_SETS,
        110,
        "friends",
        vec![],
    );
    let alice_fs2 = set_event(
        &"c".repeat(64),
        ALICE,
        KIND_FOLLOW_SETS,
        120,
        "family",
        vec![],
    );
    let bob_mute = list_event(&"d".repeat(64), BOB, KIND_MUTE_LIST, 130, vec![], "");

    for ev in [&alice_mute, &alice_fs1, &alice_fs2, &bob_mute] {
        decode_and_route(ev, &handle).expect("decode_and_route ok for valid kinds");
    }

    let alice_all = list_by_author(&handle, ALICE).unwrap();
    assert_eq!(alice_all.len(), 3, "alice has 1 mute + 2 follow-sets");

    let alice_follow_sets = list_by_author_kind(&handle, ALICE, KIND_FOLLOW_SETS).unwrap();
    assert_eq!(alice_follow_sets.len(), 2, "exactly the 2 follow-sets");
    let d_tags: Vec<&str> = alice_follow_sets.iter().map(|r| r.d_tag.as_str()).collect();
    assert!(d_tags.contains(&"friends"));
    assert!(d_tags.contains(&"family"));

    let alice_mutes = list_by_author_kind(&handle, ALICE, KIND_MUTE_LIST).unwrap();
    assert_eq!(alice_mutes.len(), 1);

    let bob_mutes = list_by_author_kind(&handle, BOB, KIND_MUTE_LIST).unwrap();
    assert_eq!(bob_mutes.len(), 1);

    assert_eq!(list_all(&handle).unwrap().len(), 4, "four lists total");
}

#[test]
fn same_author_different_kind_do_not_collide() {
    // Mute list and relay list both have d_tag == "" — composite key MUST keep
    // them as separate primary rows.
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let mute = list_event(
        &"a".repeat(64),
        ALICE,
        KIND_MUTE_LIST,
        100,
        vec![vec!["p".into(), "x".into()]],
        "",
    );
    let relay = list_event(
        &"b".repeat(64),
        ALICE,
        10002,
        100,
        vec![vec!["r".into(), "wss://a".into()]],
        "",
    );
    decode_and_route(&mute, &handle).unwrap();
    decode_and_route(&relay, &handle).unwrap();

    assert!(get(&handle, ALICE, KIND_MUTE_LIST, "").unwrap().is_some());
    assert!(get(&handle, ALICE, 10002, "").unwrap().is_some());
    assert_eq!(list_all(&handle).unwrap().len(), 2, "two distinct rows");
}

#[test]
fn stale_redelivery_replaceable_kind_10000() {
    // D4 / replaceable correctness for a 1000x replaceable list.
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let newer = list_event(
        &"b".repeat(64),
        ALICE,
        KIND_MUTE_LIST,
        200,
        vec![vec!["p".into(), "new".into()]],
        "",
    );
    let older = list_event(
        &"a".repeat(64),
        ALICE,
        KIND_MUTE_LIST,
        100,
        vec![vec!["p".into(), "old".into()]],
        "",
    );

    decode_and_route(&newer, &handle).unwrap();
    decode_and_route(&older, &handle).unwrap(); // stale redelivery

    let rec = get(&handle, ALICE, KIND_MUTE_LIST, "").unwrap().unwrap();
    assert_eq!(
        rec.created_at, 200,
        "newer mute list survives stale redelivery"
    );
    assert_eq!(rec.event_id, "b".repeat(64));
    assert_eq!(rec.items.pubkeys, vec!["new"]);
    assert_eq!(list_all(&handle).unwrap().len(), 1, "one primary row");
}

#[test]
fn stale_redelivery_parameterized_kind_30000() {
    // D4 / NIP-33 correctness for a 3000x parameterized set.
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let newer = set_event(
        &"b".repeat(64),
        ALICE,
        KIND_FOLLOW_SETS,
        200,
        "friends",
        vec![vec!["p".into(), "new".into()]],
    );
    let older = set_event(
        &"a".repeat(64),
        ALICE,
        KIND_FOLLOW_SETS,
        100,
        "friends",
        vec![vec!["p".into(), "old".into()]],
    );

    decode_and_route(&newer, &handle).unwrap();
    decode_and_route(&older, &handle).unwrap(); // stale redelivery

    let rec = get(&handle, ALICE, KIND_FOLLOW_SETS, "friends")
        .unwrap()
        .unwrap();
    assert_eq!(
        rec.created_at, 200,
        "newer follow-set survives stale redelivery"
    );
    assert_eq!(rec.event_id, "b".repeat(64));

    // In-order: a genuinely newer revision still wins.
    let newest = set_event(
        &"c".repeat(64),
        ALICE,
        KIND_FOLLOW_SETS,
        300,
        "friends",
        vec![],
    );
    decode_and_route(&newest, &handle).unwrap();
    let after = get(&handle, ALICE, KIND_FOLLOW_SETS, "friends")
        .unwrap()
        .unwrap();
    assert_eq!(after.created_at, 300);
    assert_eq!(after.event_id, "c".repeat(64));

    assert_eq!(
        list_all(&handle).unwrap().len(),
        1,
        "one (author,kind,d) row"
    );
}

#[test]
fn decode_and_route_is_a_noop_for_wrong_kind() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();
    let kind_one = stored(
        &"a".repeat(64),
        ALICE,
        1,
        0,
        vec![vec!["d".into(), "x".into()]],
        "",
    );
    decode_and_route(&kind_one, &handle).unwrap();
    assert!(
        list_all(&handle).unwrap().is_empty(),
        "kind:1 must not enter the store"
    );
}

#[test]
fn list_by_author_empty_when_author_absent() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();
    assert!(list_by_author(&handle, "nobody").unwrap().is_empty());
    assert!(list_by_author_kind(&handle, "nobody", KIND_MUTE_LIST)
        .unwrap()
        .is_empty());
}
