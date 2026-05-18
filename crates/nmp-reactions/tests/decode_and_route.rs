//! Domain-store integration: `decode_and_route` writes the reverse indexes;
//! `list_for_target` returns every reaction for an event; wrong kind is a
//! no-op.

mod common;

use common::{reaction, stored};
use nmp_core::store::{EventStore, MemEventStore};
use nmp_reactions::{
    decode_and_route, list_by_reactor, list_for_target, ReactionTarget, NAMESPACE,
};

const TARGET: &str = "target-event-id-000000000000000000000000000000000000000000000000000";
const TARGET_AUTHOR: &str = "target-author-00000000000000000000000000000000000000000000000000";
const ALICE: &str = "alice-000000000000000000000000000000000000000000000000000000000000";
const BOB: &str = "bob-00000000000000000000000000000000000000000000000000000000000000";

#[test]
fn decode_and_route_writes_indexes_list_for_target_returns_all() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).expect("namespace opens");

    let r1 = reaction(&"a".repeat(64), ALICE, 100, TARGET, TARGET_AUTHOR, "👍");
    let r2 = reaction(&"b".repeat(64), BOB, 200, TARGET, TARGET_AUTHOR, "❤️");

    decode_and_route(&r1, &handle).expect("route r1");
    decode_and_route(&r2, &handle).expect("route r2");

    let target = ReactionTarget::Event(TARGET.to_string());
    let listed = list_for_target(&handle, &target).expect("list_for_target");
    assert_eq!(listed.len(), 2);
    // newest-first.
    assert_eq!(listed[0].event_id, "b".repeat(64));
    assert_eq!(listed[1].event_id, "a".repeat(64));

    let by_alice = list_by_reactor(&handle, ALICE).expect("list_by_reactor");
    assert_eq!(by_alice.len(), 1);
    assert_eq!(by_alice[0].author, ALICE);
}

#[test]
fn decode_and_route_is_a_noop_for_wrong_kind() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let note = stored(
        &"a".repeat(64),
        ALICE,
        1,
        0,
        vec![vec!["e".into(), TARGET.into()]],
        "just a note",
    );
    decode_and_route(&note, &handle).unwrap();

    let target = ReactionTarget::Event(TARGET.to_string());
    let listed = list_for_target(&handle, &target).unwrap();
    assert!(
        listed.is_empty(),
        "kind:1 must not enter the reactions store"
    );
}
