//! Decoder integration: each of the six kinds decodes; wrong kind → None;
//! parameterized set missing `d` → None; replaceable kinds need no `d`;
//! encrypted content preserved verbatim.

mod common;

use common::{list_event, set_event, stored};
use nmp_nip51::{
    try_from_event, ListKind, KIND_BOOKMARK_LIST, KIND_BOOKMARK_SETS, KIND_FOLLOW_SETS,
    KIND_MUTE_LIST, KIND_RELAY_LIST, KIND_RELAY_SETS,
};

const ALICE: &str = "alice-pubkey-0000000000000000000000000000000000000000000000000000000";

#[test]
fn replaceable_kinds_decode_without_d_tag() {
    for (kind, expect) in [
        (KIND_MUTE_LIST, ListKind::Mute),
        (KIND_RELAY_LIST, ListKind::RelayList),
        (KIND_BOOKMARK_LIST, ListKind::Bookmark),
    ] {
        let ev = list_event(&"a".repeat(64), ALICE, kind, 100, vec![], "");
        let rec = try_from_event(&ev).expect("replaceable list decodes");
        assert_eq!(rec.list_kind, expect);
        assert_eq!(rec.d_tag, "");
    }
}

#[test]
fn parameterized_kinds_decode_with_d_tag() {
    for (kind, expect) in [
        (KIND_FOLLOW_SETS, ListKind::FollowSet),
        (KIND_RELAY_SETS, ListKind::RelaySet),
        (KIND_BOOKMARK_SETS, ListKind::BookmarkSet),
    ] {
        let ev = set_event(&"a".repeat(64), ALICE, kind, 100, "myset", vec![]);
        let rec = try_from_event(&ev).expect("set decodes with d");
        assert_eq!(rec.list_kind, expect);
        assert_eq!(rec.d_tag, "myset");
    }
}

#[test]
fn wrong_kind_returns_none() {
    let ev = stored(&"a".repeat(64), ALICE, 1, 0, vec![], "");
    assert!(try_from_event(&ev).is_none());
    // 10001 (pin list) is NIP-51 but explicitly out of this crate's scope.
    let pin = stored(&"a".repeat(64), ALICE, 10001, 0, vec![], "");
    assert!(try_from_event(&pin).is_none());
}

#[test]
fn parameterized_missing_d_returns_none() {
    let ev = stored(
        &"a".repeat(64),
        ALICE,
        KIND_FOLLOW_SETS,
        0,
        vec![vec!["p".into(), "pk".into()]],
        "",
    );
    assert!(try_from_event(&ev).is_none());
}

#[test]
fn item_extraction_covers_p_e_a_t_r_word() {
    let ev = list_event(
        &"a".repeat(64),
        ALICE,
        KIND_BOOKMARK_LIST,
        0,
        vec![
            vec!["p".into(), "pk1".into()],
            vec!["e".into(), "ev1".into()],
            vec!["a".into(), "30023:pk:slug".into()],
            vec!["t".into(), "nostr".into()],
            vec!["r".into(), "wss://relay".into(), "read".into()],
            vec!["word".into(), "spam".into()],
        ],
        "",
    );
    let rec = try_from_event(&ev).unwrap();
    assert_eq!(rec.items.pubkeys, vec!["pk1"]);
    assert_eq!(rec.items.events, vec!["ev1"]);
    assert_eq!(rec.items.addresses, vec!["30023:pk:slug"]);
    assert_eq!(rec.items.hashtags, vec!["nostr"]);
    assert_eq!(rec.items.relays[0].url, "wss://relay");
    assert_eq!(rec.items.relays[0].marker.as_deref(), Some("read"));
    assert_eq!(rec.items.words, vec!["spam"]);
}

#[test]
fn set_metadata_title_description_image_surface() {
    let ev = set_event(
        &"a".repeat(64),
        ALICE,
        KIND_FOLLOW_SETS,
        0,
        "friends",
        vec![
            vec!["title".into(), "Close Friends".into()],
            vec!["description".into(), "people I trust".into()],
            vec!["image".into(), "https://example.com/i.png".into()],
        ],
    );
    let rec = try_from_event(&ev).unwrap();
    assert_eq!(rec.title.as_deref(), Some("Close Friends"));
    assert_eq!(rec.description.as_deref(), Some("people I trust"));
    assert_eq!(rec.image.as_deref(), Some("https://example.com/i.png"));
}

#[test]
fn encrypted_content_preserved_verbatim_not_decrypted() {
    let cipher = "k0nT3nT==?iv=YWJj==";
    let ev = list_event(
        &"a".repeat(64),
        ALICE,
        KIND_MUTE_LIST,
        0,
        vec![vec!["p".into(), "public-muted".into()]],
        cipher,
    );
    let rec = try_from_event(&ev).unwrap();
    assert_eq!(rec.encrypted_payload, cipher, "ciphertext byte-for-byte");
    // Public entry still decoded; private payload untouched.
    assert_eq!(rec.items.pubkeys, vec!["public-muted"]);
}

#[test]
fn tags_preserved_verbatim() {
    let tags = vec![
        vec!["d".into(), "s".into()],
        vec!["p".into(), "pk".into(), "wss://hint".into()],
        vec!["x-client".into(), "custom".into()],
    ];
    let ev = stored(&"a".repeat(64), ALICE, KIND_RELAY_SETS, 0, tags.clone(), "");
    let rec = try_from_event(&ev).unwrap();
    assert_eq!(rec.tags, tags);
}
