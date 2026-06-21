//! Round-trip + fail-closed tests for the nip02 follow-action typed payload
//! codecs (ADR-0064 / S3 #1751). Every fail-closed gate asserts the NEGATIVE.

use super::*;
use crate::{FollowManyAction, PubkeyAction};
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

#[test]
fn pubkey_action_round_trips() {
    let action = PubkeyAction { pubkey: "a".repeat(64) };
    let decoded = PubkeyAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn follow_many_round_trips() {
    let action = FollowManyAction {
        pubkeys: vec!["a".repeat(64), "b".repeat(64), "c".repeat(64)],
    };
    let decoded = FollowManyAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn follow_many_empty_round_trips() {
    let action = FollowManyAction { pubkeys: vec![] };
    let decoded = FollowManyAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn pubkey_action_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let pubkey = fbb.create_string(&"a".repeat(64));
    let payload = follow_fb::FollowActionPayload::create(
        &mut fbb,
        &follow_fb::FollowActionPayloadArgs {
            schema_version: 999,
            pubkey: Some(pubkey),
        },
    );
    follow_fb::finish_follow_action_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = PubkeyAction::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch { found: 999, expected: SCHEMA_VERSION }
    );
}

#[test]
fn follow_many_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let pubkeys = fbb.create_vector::<flatbuffers::WIPOffset<&str>>(&[]);
    let payload = follow_many_fb::FollowManyActionPayload::create(
        &mut fbb,
        &follow_many_fb::FollowManyActionPayloadArgs {
            schema_version: 2,
            pubkeys: Some(pubkeys),
        },
    );
    follow_many_fb::finish_follow_many_action_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = FollowManyAction::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch { found: 2, expected: SCHEMA_VERSION }
    );
}

#[test]
fn malformed_buffers_are_rejected() {
    assert!(matches!(
        PubkeyAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        FollowManyAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
