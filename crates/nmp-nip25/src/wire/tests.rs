//! Round-trip + fail-closed tests for the nip25 typed payload codecs
//! (ADR-0071 / S3 #1751). Every fail-closed gate asserts the NEGATIVE.

use super::*;
use crate::action::{ReactAction, UnreactAction};
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

#[test]
fn react_round_trips_with_author() {
    let action = ReactAction {
        target_event_id: "a".repeat(64),
        reaction: "🔥".to_string(),
        target_author_pubkey: Some("b".repeat(64)),
    };
    let decoded = ReactAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn react_round_trips_without_author() {
    let action = ReactAction {
        target_event_id: "c".repeat(64),
        reaction: "+".to_string(),
        target_author_pubkey: None,
    };
    let decoded = ReactAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
    assert!(decoded.target_author_pubkey.is_none());
}

#[test]
fn unreact_round_trips() {
    let action = UnreactAction {
        reaction_event_id: "d".repeat(64),
        reason: "undo".to_string(),
    };
    let decoded = UnreactAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn react_wrong_schema_version_is_rejected() {
    // Hand-build a ReactPayload with a bogus schema_version.
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let target = fbb.create_string(&"a".repeat(64));
    let reaction = fbb.create_string("+");
    let payload = react_fb::ReactPayload::create(
        &mut fbb,
        &react_fb::ReactPayloadArgs {
            schema_version: 999,
            target_event_id: Some(target),
            reaction: Some(reaction),
            target_author_pubkey: None,
        },
    );
    react_fb::finish_react_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = ReactAction::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}

#[test]
fn unreact_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let id = fbb.create_string(&"d".repeat(64));
    let reason = fbb.create_string("");
    let payload = unreact_fb::UnreactPayload::create(
        &mut fbb,
        &unreact_fb::UnreactPayloadArgs {
            schema_version: 7,
            reaction_event_id: Some(id),
            reason: Some(reason),
        },
    );
    unreact_fb::finish_unreact_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = UnreactAction::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 7,
            expected: SCHEMA_VERSION
        }
    );
}

#[test]
fn malformed_buffers_are_rejected() {
    assert!(matches!(
        ReactAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        UnreactAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
