//! Domain-store integration for `nmp.nip57.zaps`.
//!
//! `decode::try_from_event` + `domain::decode_and_route` only had key-encoding
//! unit tests — the actual routing/listing round-trip through a `DomainHandle`
//! was uncovered. These integration tests pin:
//!
//! - a zap receipt with an `e` tag is reverse-indexed under its zapped note
//!   and shows up in `list_by_target`,
//! - a profile zap (no `e` tag) is a no-op — it must not enter the by-target
//!   index (siblings would own profile/address aggregation),
//! - a non-kind-9735 event is a no-op,
//! - multiple receipts for the same note all enumerate distinctly,
//! - `list_by_target` on an unknown note is empty (scan on empty store),
//! - distinct notes do not cross-talk.

use nmp_core::store::{EventStore, MemEventStore, RawEvent, StoredEvent};
use nmp_nip57::{decode_and_route, list_by_target, NAMESPACE};
use std::sync::Arc;

const RECIPIENT: &str = "recipient-pk-00000000000000000000000000000000000000000000000000000";
const NOTE: &str = "zapped-note-id-0000000000000000000000000000000000000000000000000000";
const OTHER_NOTE: &str = "other-note-id-00000000000000000000000000000000000000000000000000000";

fn stored(id: &str, kind: u32, tags: Vec<Vec<String>>) -> StoredEvent {
    StoredEvent {
        raw: Arc::new(RawEvent {
            id: id.into(),
            pubkey: "ln-node-pk".into(),
            created_at: 1_700_000_000,
            kind,
            tags,
            content: String::new(),
            sig: "0".repeat(128),
        }),
        received_at_ms: 0,
    }
}

/// kind:9735 zap receipt targeting a note: `p` recipient + `e` zapped note.
fn receipt_for_note(id: &str, note: &str) -> StoredEvent {
    stored(
        id,
        9735,
        vec![
            vec!["p".into(), RECIPIENT.into()],
            vec!["e".into(), note.into()],
            vec!["bolt11".into(), "lnbc21n1pvj...".into()],
        ],
    )
}

#[test]
fn receipt_is_indexed_under_its_zapped_note_and_listed() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).expect("namespace opens");

    let r = receipt_for_note(&"a".repeat(64), NOTE);
    decode_and_route(&r, &handle).expect("route receipt");

    let under_note = list_by_target(&handle, NOTE).expect("list_by_target");
    assert_eq!(under_note, vec!["a".repeat(64)]);
}

#[test]
fn profile_zap_without_e_tag_is_a_noop() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    // A direct profile zap names only the `p` recipient — no `e` note ref.
    let profile_zap = stored(
        &"b".repeat(64),
        9735,
        vec![
            vec!["p".into(), RECIPIENT.into()],
            vec!["bolt11".into(), "lnbc21n1pvj...".into()],
        ],
    );
    decode_and_route(&profile_zap, &handle).expect("route is infallible for a profile zap");

    // It must not appear under the recipient id, nor anywhere else.
    assert!(
        list_by_target(&handle, RECIPIENT).unwrap().is_empty(),
        "a profile zap must not enter the by-note index"
    );
}

#[test]
fn non_kind_9735_event_is_a_noop() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    // A kind:9734 zap *request* that happens to carry an `e` tag — it is not a
    // receipt and must not be indexed.
    let request = stored(
        &"c".repeat(64),
        9734,
        vec![
            vec!["p".into(), RECIPIENT.into()],
            vec!["e".into(), NOTE.into()],
        ],
    );
    decode_and_route(&request, &handle).unwrap();

    assert!(
        list_by_target(&handle, NOTE).unwrap().is_empty(),
        "only kind:9735 zap receipts enter the index"
    );
}

#[test]
fn multiple_receipts_for_same_note_all_enumerate() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let r1 = receipt_for_note(&"1".repeat(64), NOTE);
    let r2 = receipt_for_note(&"2".repeat(64), NOTE);
    let r3 = receipt_for_note(&"3".repeat(64), NOTE);
    for r in [&r1, &r2, &r3] {
        decode_and_route(r, &handle).unwrap();
    }

    let mut listed = list_by_target(&handle, NOTE).expect("list_by_target");
    listed.sort();
    assert_eq!(listed, vec!["1".repeat(64), "2".repeat(64), "3".repeat(64)]);

    // A different note is isolated — no cross-talk.
    assert!(list_by_target(&handle, OTHER_NOTE).unwrap().is_empty());
}

#[test]
fn list_by_target_is_empty_for_unknown_note() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let listed =
        list_by_target(&handle, "nobody-zapped-this").expect("scan on empty store works");
    assert!(listed.is_empty());
}
