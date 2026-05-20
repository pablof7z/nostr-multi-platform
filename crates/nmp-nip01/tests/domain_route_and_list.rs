//! Domain-store integration for `nmp.nip01.replies`.
//!
//! `decode::try_from_event` + `domain::decode_and_route` only had key-encoding
//! unit tests — the actual routing/listing round-trip through a `DomainHandle`
//! was uncovered. These integration tests pin:
//!
//! - a reply is reverse-indexed under its NIP-10 `reply` parent and shows up
//!   in `list_by_parent`,
//! - a thread root (no reply pointer) is a no-op — it must not self-index,
//! - non-kind-1 events are a no-op,
//! - multiple replies to the same parent all enumerate,
//! - `list_by_parent` on an unknown parent is empty (scan on empty store),
//! - the parent for routing is the NIP-10 `reply` marker, not `root`.

use nmp_core::store::{EventStore, MemEventStore, RawEvent, StoredEvent};
use nmp_nip01::{decode_and_route, list_by_parent, NAMESPACE};
use std::sync::Arc;

const ALICE: &str = "alice-pubkey-0000000000000000000000000000000000000000000000000000000";
const ROOT: &str = "root-event-id-000000000000000000000000000000000000000000000000000000";
const PARENT: &str = "parent-event-0000000000000000000000000000000000000000000000000000000";

fn stored(id: &str, kind: u32, tags: Vec<Vec<String>>, content: &str) -> StoredEvent {
    StoredEvent {
        raw: Arc::new(RawEvent {
            id: id.into(),
            pubkey: ALICE.into(),
            created_at: 1_700_000_000,
            kind,
            tags,
            content: content.into(),
            sig: "0".repeat(128),
        }),
        received_at_ms: 0,
    }
}

/// kind:1 reply: NIP-10 marked-form `root` + `reply` `e` tags.
fn reply(id: &str, root: &str, parent: &str) -> StoredEvent {
    stored(
        id,
        1,
        vec![
            vec!["e".into(), root.into(), "".into(), "root".into()],
            vec!["e".into(), parent.into(), "".into(), "reply".into()],
        ],
        "a reply",
    )
}

#[test]
fn reply_is_indexed_under_its_parent_and_listed() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).expect("namespace opens");

    let r = reply(&"a".repeat(64), ROOT, PARENT);
    decode_and_route(&r, &handle).expect("route reply");

    let under_parent = list_by_parent(&handle, PARENT).expect("list_by_parent");
    assert_eq!(under_parent, vec!["a".repeat(64)]);
}

#[test]
fn thread_root_is_a_noop_does_not_self_index() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    // A root note carries no NIP-10 reply pointer.
    let root = stored(ROOT, 1, vec![], "i am a root note");
    decode_and_route(&root, &handle).expect("route is infallible for a root");

    // It must not appear under its own id, nor anywhere else.
    assert!(
        list_by_parent(&handle, ROOT).unwrap().is_empty(),
        "a thread root must not be reverse-indexed"
    );
}

#[test]
fn non_kind_1_event_is_a_noop() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    // kind:7 reaction that happens to carry an `e` reply marker.
    let reaction = stored(
        &"e".repeat(64),
        7,
        vec![vec!["e".into(), PARENT.into(), "".into(), "reply".into()]],
        "+",
    );
    decode_and_route(&reaction, &handle).unwrap();

    assert!(
        list_by_parent(&handle, PARENT).unwrap().is_empty(),
        "only kind:1 events enter the replies index"
    );
}

#[test]
fn multiple_replies_to_same_parent_all_enumerate() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let r1 = reply(&"1".repeat(64), ROOT, PARENT);
    let r2 = reply(&"2".repeat(64), ROOT, PARENT);
    let r3 = reply(&"3".repeat(64), ROOT, PARENT);
    for r in [&r1, &r2, &r3] {
        decode_and_route(r, &handle).unwrap();
    }

    let mut listed = list_by_parent(&handle, PARENT).expect("list_by_parent");
    listed.sort();
    assert_eq!(listed, vec!["1".repeat(64), "2".repeat(64), "3".repeat(64)]);

    // A different parent is isolated — no cross-talk.
    assert!(list_by_parent(&handle, ROOT).unwrap().is_empty());
}

#[test]
fn list_by_parent_is_empty_for_unknown_parent() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let listed = list_by_parent(&handle, "nobody-has-this-id").expect("scan on empty store works");
    assert!(listed.is_empty());
}

#[test]
fn routing_keys_on_the_reply_marker_not_the_root_marker() {
    // A mid-thread reply: root != reply. The index entry must land under the
    // direct `reply` parent so a UI listing direct replies of PARENT finds it,
    // and a listing of ROOT's direct replies does NOT (this note is nested).
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let nested = reply(&"d".repeat(64), ROOT, PARENT);
    decode_and_route(&nested, &handle).unwrap();

    assert_eq!(
        list_by_parent(&handle, PARENT).unwrap(),
        vec!["d".repeat(64)],
        "indexed under the direct reply parent"
    );
    assert!(
        list_by_parent(&handle, ROOT).unwrap().is_empty(),
        "a nested reply is not a direct reply of the thread root"
    );
}
