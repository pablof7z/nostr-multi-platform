//! Round-trip + fail-closed tests for the nip51 bookmark-update typed payload
//! codec (ADR-0064 / S9 #1747). Every fail-closed gate asserts the NEGATIVE,
//! and every `BookmarkItem` variant round-trips through the tagged table.

use super::*;
use crate::bookmarks::{BookmarkItem, BookmarkUpdateInput};
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

fn account() -> String {
    "ab".repeat(32)
}

fn round_trip(item: BookmarkItem) {
    let action = BookmarkUpdateInput {
        account_pubkey: account(),
        item,
    };
    let decoded = BookmarkUpdateInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn event_item_round_trips_with_relay() {
    round_trip(BookmarkItem::Event {
        event_id: "cd".repeat(32),
        relay: Some("wss://relay.example".to_string()),
    });
}

#[test]
fn event_item_round_trips_without_relay() {
    round_trip(BookmarkItem::Event {
        event_id: "cd".repeat(32),
        relay: None,
    });
}

#[test]
fn address_item_round_trips_with_relay() {
    round_trip(BookmarkItem::Address {
        coordinate: format!("30023:{}:my-article", "ef".repeat(32)),
        relay: Some("wss://relay.example".to_string()),
    });
}

#[test]
fn address_item_round_trips_without_relay() {
    round_trip(BookmarkItem::Address {
        coordinate: format!("30023:{}:my-article", "ef".repeat(32)),
        relay: None,
    });
}

#[test]
fn url_item_round_trips() {
    round_trip(BookmarkItem::Url {
        url: "https://example.com/article".to_string(),
    });
}

#[test]
fn hashtag_item_round_trips() {
    round_trip(BookmarkItem::Hashtag {
        hashtag: "nostr".to_string(),
    });
}

#[test]
fn empty_relay_presence_is_preserved_not_collapsed() {
    // The nip57 lesson: Some("") must NOT collapse to None on this codec.
    // Normalization stays in the actor's command handler.
    let action = BookmarkUpdateInput {
        account_pubkey: account(),
        item: BookmarkItem::Event {
            event_id: "cd".repeat(32),
            relay: Some(String::new()),
        },
    };
    let decoded = BookmarkUpdateInput::decode(&action.encode()).expect("decodes");
    match decoded.item {
        BookmarkItem::Event { relay, .. } => {
            assert_eq!(
                relay,
                Some(String::new()),
                "empty relay must stay Some(\"\")"
            );
        }
        other => panic!("expected Event item, got {other:?}"),
    }
}

#[test]
fn wrong_schema_version_is_rejected() {
    // Hand-build a BookmarkUpdatePayload with a bogus schema_version.
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
    let payload = fb::BookmarkUpdatePayload::create(
        &mut fbb,
        &fb::BookmarkUpdatePayloadArgs {
            schema_version: 999,
            account_pubkey: Some(account_pubkey),
            item: Some(item),
        },
    );
    fb::finish_bookmark_update_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = BookmarkUpdateInput::decode(&bytes).expect_err("bad version rejected");
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
        BookmarkUpdateInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        BookmarkUpdateInput::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn wrong_identifier_is_rejected() {
    // Encode a well-formed payload then corrupt the file-identifier bytes.
    let action = BookmarkUpdateInput {
        account_pubkey: account(),
        item: BookmarkItem::Url {
            url: "https://example.com".to_string(),
        },
    };
    let mut bytes = action.encode();
    // The file identifier sits at bytes[4..8].
    bytes[4] = b'X';
    bytes[5] = b'X';
    bytes[6] = b'X';
    bytes[7] = b'X';
    assert!(matches!(
        BookmarkUpdateInput::decode(&bytes),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
