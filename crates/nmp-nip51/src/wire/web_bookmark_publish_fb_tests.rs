//! Round-trip + fail-closed tests for the web-bookmark publish typed payload.

use super::*;
use crate::web_bookmarks::{PublishWebBookmarkInput, WebBookmarkDraft};
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

fn account() -> String {
    "ab".repeat(32)
}

#[test]
fn full_payload_round_trips() {
    let action = PublishWebBookmarkInput {
        account_pubkey: account(),
        bookmark: WebBookmarkDraft {
            url: "https://alice.blog/post".to_string(),
            title: Some("Blog insights".to_string()),
            description: Some("A useful article.".to_string()),
            published_at: Some(0),
            hashtags: vec!["nostr".to_string(), "writing".to_string()],
        },
    };
    let decoded = PublishWebBookmarkInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn minimal_payload_round_trips() {
    let action = PublishWebBookmarkInput {
        account_pubkey: account(),
        bookmark: WebBookmarkDraft {
            url: "https://alice.blog/post".to_string(),
            title: None,
            description: None,
            published_at: None,
            hashtags: Vec::new(),
        },
    };
    let decoded = PublishWebBookmarkInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn empty_optional_strings_are_preserved_not_collapsed() {
    let action = PublishWebBookmarkInput {
        account_pubkey: account(),
        bookmark: WebBookmarkDraft {
            url: "https://alice.blog/post".to_string(),
            title: Some(String::new()),
            description: Some(String::new()),
            published_at: None,
            hashtags: Vec::new(),
        },
    };
    let decoded = PublishWebBookmarkInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded.bookmark.title, Some(String::new()));
    assert_eq!(decoded.bookmark.description, Some(String::new()));
}

#[test]
fn wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let account_pubkey = fbb.create_string(&account());
    let url = fbb.create_string("https://alice.blog/post");
    let payload = fb::WebBookmarkPublishPayload::create(
        &mut fbb,
        &fb::WebBookmarkPublishPayloadArgs {
            schema_version: 999,
            account_pubkey: Some(account_pubkey),
            url: Some(url),
            title: None,
            description: None,
            published_at: 0,
            has_published_at: false,
            hashtags: None,
        },
    );
    fb::finish_web_bookmark_publish_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = PublishWebBookmarkInput::decode(&bytes).expect_err("bad version rejected");
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
        PublishWebBookmarkInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        PublishWebBookmarkInput::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn wrong_identifier_is_rejected() {
    let action = PublishWebBookmarkInput {
        account_pubkey: account(),
        bookmark: WebBookmarkDraft {
            url: "https://alice.blog/post".to_string(),
            title: None,
            description: None,
            published_at: None,
            hashtags: Vec::new(),
        },
    };
    let mut bytes = action.encode();
    bytes[4] = b'X';
    bytes[5] = b'X';
    bytes[6] = b'X';
    bytes[7] = b'X';
    assert!(matches!(
        PublishWebBookmarkInput::decode(&bytes),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
