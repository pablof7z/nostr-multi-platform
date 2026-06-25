//! Round-trip + fail-closed tests for the bookmark-set update typed payload.

use super::*;
use crate::bookmark_sets::{BookmarkSetKind, BookmarkSetUpdateInput};
use crate::bookmarks::BookmarkItem;
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

fn account() -> String {
    "ab".repeat(32)
}

fn round_trip(set_kind: BookmarkSetKind, item: BookmarkItem) {
    let action = BookmarkSetUpdateInput {
        account_pubkey: account(),
        set_kind,
        identifier: "reading".to_string(),
        item,
    };
    let decoded = BookmarkSetUpdateInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn bookmark_set_event_item_round_trips_with_relay() {
    round_trip(
        BookmarkSetKind::BookmarkSet,
        BookmarkItem::Event {
            event_id: "cd".repeat(32),
            relay: Some("wss://relay.example".to_string()),
        },
    );
}

#[test]
fn curation_set_address_item_round_trips_without_relay() {
    round_trip(
        BookmarkSetKind::CurationSet,
        BookmarkItem::Address {
            coordinate: format!("30023:{}:my-article", "ef".repeat(32)),
            relay: None,
        },
    );
}

#[test]
fn url_and_hashtag_items_round_trip() {
    round_trip(
        BookmarkSetKind::BookmarkSet,
        BookmarkItem::Url {
            url: "https://example.com/article".to_string(),
        },
    );
    round_trip(
        BookmarkSetKind::CurationSet,
        BookmarkItem::Hashtag {
            hashtag: "nostr".to_string(),
        },
    );
}

#[test]
fn empty_relay_presence_is_preserved_not_collapsed() {
    let action = BookmarkSetUpdateInput {
        account_pubkey: account(),
        set_kind: BookmarkSetKind::BookmarkSet,
        identifier: "reading".to_string(),
        item: BookmarkItem::Event {
            event_id: "cd".repeat(32),
            relay: Some(String::new()),
        },
    };
    let decoded = BookmarkSetUpdateInput::decode(&action.encode()).expect("decodes");
    match decoded.item {
        BookmarkItem::Event { relay, .. } => assert_eq!(relay, Some(String::new())),
        other => panic!("expected Event item, got {other:?}"),
    }
}

#[test]
fn wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let value = fbb.create_string("cd".repeat(32).as_str());
    let item = fb::BookmarkItem::create(
        &mut fbb,
        &fb::BookmarkItemArgs {
            kind: fb::BookmarkItemKind::Event,
            value: Some(value),
            relay: None,
        },
    );
    let account_pubkey = fbb.create_string(&account());
    let identifier = fbb.create_string("reading");
    let payload = fb::BookmarkSetUpdatePayload::create(
        &mut fbb,
        &fb::BookmarkSetUpdatePayloadArgs {
            schema_version: 999,
            account_pubkey: Some(account_pubkey),
            set_kind: fb::BookmarkSetKindWire::BookmarkSet,
            identifier: Some(identifier),
            item: Some(item),
        },
    );
    fb::finish_bookmark_set_update_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = BookmarkSetUpdateInput::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}

#[test]
fn malformed_buffers_are_rejected() {
    assert!(matches!(
        BookmarkSetUpdateInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        BookmarkSetUpdateInput::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn wrong_identifier_is_rejected() {
    let action = BookmarkSetUpdateInput {
        account_pubkey: account(),
        set_kind: BookmarkSetKind::BookmarkSet,
        identifier: "reading".to_string(),
        item: BookmarkItem::Url {
            url: "https://example.com".to_string(),
        },
    };
    let mut bytes = action.encode();
    bytes[4] = b'X';
    bytes[5] = b'X';
    bytes[6] = b'X';
    bytes[7] = b'X';
    assert!(matches!(
        BookmarkSetUpdateInput::decode(&bytes),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
