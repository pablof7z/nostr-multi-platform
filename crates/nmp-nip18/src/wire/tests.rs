use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::RepostAction;

#[test]
fn repost_payload_round_trips() {
    let action = RepostAction {
        target_event_id: "a".repeat(64),
        target_kind: 1,
        target_author_pubkey: Some("b".repeat(64)),
        relay_hint: Some("wss://relay.example".to_string()),
    };

    let decoded = RepostAction::decode(&action.encode()).expect("payload decodes");
    assert_eq!(decoded, action);
}

#[test]
fn repost_payload_rejects_wrong_file_identifier() {
    let err = RepostAction::decode(b"not-flatbuffers").expect_err("bad bytes fail");
    assert!(matches!(err, ActionPayloadDecodeError::Malformed { .. }));
}
