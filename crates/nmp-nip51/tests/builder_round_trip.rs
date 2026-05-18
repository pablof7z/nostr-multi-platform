//! Builder → fake-stored → `try_from_event` round trips for every list type,
//! and `MissingDTag` enforcement for the three set kinds.

mod common;

use common::stored;
use nmp_nip51::{
    try_from_event, BookmarkList, BookmarkSet, FollowSet, ListKind, MuteList, Nip51BuildError,
    RelayList, RelaySet,
};

const AUTHOR: &str = "b-pubkey-00000000000000000000000000000000000000000000000000000000000000";

#[test]
fn mute_list_round_trips() {
    let unsigned = MuteList::builder()
        .pubkey("spammer")
        .word("airdrop")
        .build(AUTHOR, 1_700_000_000)
        .unwrap();
    let ev = stored(
        &"a".repeat(64),
        &unsigned.pubkey,
        unsigned.kind,
        unsigned.created_at,
        unsigned.tags,
        &unsigned.content,
    );
    let rec = try_from_event(&ev).unwrap();
    assert_eq!(rec.list_kind, ListKind::Mute);
    assert_eq!(rec.d_tag, "");
    assert_eq!(rec.items.pubkeys, vec!["spammer"]);
    assert_eq!(rec.items.words, vec!["airdrop"]);
    assert_eq!(rec.encrypted_payload, "", "builder leaves content empty");
}

#[test]
fn relay_list_round_trips_with_markers() {
    let unsigned = RelayList::builder()
        .relay("wss://a", Some("write".into()))
        .relay("wss://b", None)
        .build(AUTHOR, 0)
        .unwrap();
    let ev = stored(
        &"a".repeat(64),
        &unsigned.pubkey,
        unsigned.kind,
        unsigned.created_at,
        unsigned.tags,
        &unsigned.content,
    );
    let rec = try_from_event(&ev).unwrap();
    assert_eq!(rec.list_kind, ListKind::RelayList);
    assert_eq!(rec.items.relays.len(), 2);
    assert_eq!(rec.items.relays[0].marker.as_deref(), Some("write"));
    assert_eq!(rec.items.relays[1].marker, None);
}

#[test]
fn bookmark_list_round_trips() {
    let unsigned = BookmarkList::builder()
        .event("ev1")
        .build(AUTHOR, 5)
        .unwrap();
    let ev = stored(
        &"a".repeat(64),
        &unsigned.pubkey,
        unsigned.kind,
        unsigned.created_at,
        unsigned.tags,
        &unsigned.content,
    );
    let rec = try_from_event(&ev).unwrap();
    assert_eq!(rec.list_kind, ListKind::Bookmark);
    assert_eq!(rec.items.events, vec!["ev1"]);
}

#[test]
fn follow_set_round_trips_with_metadata() {
    let unsigned = FollowSet::new("friends")
        .title("Friends")
        .description("close")
        .pubkey("pk1")
        .build(AUTHOR, 9)
        .unwrap();
    let ev = stored(
        &"a".repeat(64),
        &unsigned.pubkey,
        unsigned.kind,
        unsigned.created_at,
        unsigned.tags,
        &unsigned.content,
    );
    let rec = try_from_event(&ev).unwrap();
    assert_eq!(rec.list_kind, ListKind::FollowSet);
    assert_eq!(rec.d_tag, "friends");
    assert_eq!(rec.title.as_deref(), Some("Friends"));
    assert_eq!(rec.description.as_deref(), Some("close"));
    assert_eq!(rec.items.pubkeys, vec!["pk1"]);
}

#[test]
fn relay_and_bookmark_sets_require_d_tag() {
    assert_eq!(
        RelaySet::new("  ").build(AUTHOR, 0).unwrap_err(),
        Nip51BuildError::MissingDTag
    );
    assert_eq!(
        BookmarkSet::new("").build(AUTHOR, 0).unwrap_err(),
        Nip51BuildError::MissingDTag
    );
    assert_eq!(
        FollowSet::new("\t\n").build(AUTHOR, 0).unwrap_err(),
        Nip51BuildError::MissingDTag
    );
}

#[test]
fn error_display_is_human_readable() {
    let msg = format!("{}", Nip51BuildError::MissingDTag);
    assert!(msg.contains("NIP-51"));
    assert!(msg.contains("`d`"));
}
