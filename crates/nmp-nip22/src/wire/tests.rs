//! Round-trip + fail-closed tests for the nip22 post_comment typed payload codec
//! (ADR-0064 / S9 #1747). Every fail-closed gate asserts the NEGATIVE.

use super::*;
use crate::action::PostCommentAction;
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

fn full_action() -> PostCommentAction {
    PostCommentAction {
        root_tag_name: "A".to_string(),
        root_tag_value: "30023:".to_string() + &"a".repeat(64) + ":my-article",
        root_kind: 30023,
        parent_event_id: Some("b".repeat(64)),
        root_author_pubkey: Some("c".repeat(64)),
        parent_author_pubkey: Some("d".repeat(64)),
        content: "Great article!".to_string(),
    }
}

fn top_level_action() -> PostCommentAction {
    PostCommentAction {
        root_tag_name: "E".to_string(),
        root_tag_value: "e".repeat(64),
        root_kind: 1,
        parent_event_id: None,
        root_author_pubkey: None,
        parent_author_pubkey: None,
        content: "Top-level comment".to_string(),
    }
}

#[test]
fn post_comment_round_trips_with_all_optional_fields() {
    let action = full_action();
    let decoded = PostCommentAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn post_comment_round_trips_without_optional_fields() {
    let action = top_level_action();
    let decoded = PostCommentAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
    assert!(decoded.parent_event_id.is_none());
    assert!(decoded.root_author_pubkey.is_none());
    assert!(decoded.parent_author_pubkey.is_none());
}

#[test]
fn post_comment_wrong_schema_version_is_rejected() {
    // Hand-build a PostComment payload with a bogus schema_version.
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let root_tag_name = fbb.create_string("A");
    let root_tag_value = fbb.create_string("30023:aaaa:d");
    let content = fbb.create_string("hello");
    let payload = fb::PostComment::create(
        &mut fbb,
        &fb::PostCommentArgs {
            schema_version: 999,
            root_tag_name: Some(root_tag_name),
            root_tag_value: Some(root_tag_value),
            root_kind: 30023,
            parent_event_id: None,
            root_author_pubkey: None,
            parent_author_pubkey: None,
            content: Some(content),
        },
    );
    fb::finish_post_comment_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = PostCommentAction::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch { found: 999, expected: SCHEMA_VERSION }
    );
}

#[test]
fn malformed_buffers_are_rejected() {
    assert!(matches!(
        PostCommentAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        PostCommentAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
