use super::*;

#[test]
fn reply_action_payload_round_trips() {
    let action = ReplyAction {
        target_event_id: Some("1".repeat(64)),
        target_kind: 1,
        target_author_pubkey: Some("2".repeat(64)),
        target_address: None,
        target_external_uri: None,
        relay_hint: Some("wss://relay.example".to_string()),
        content: "hello".to_string(),
    };

    let bytes = action.encode();
    let decoded = ReplyAction::decode(&bytes).unwrap();
    assert_eq!(decoded, action);
}

#[test]
fn wrong_file_identifier_rejects() {
    let bytes = vec![0_u8; 16];
    let err = ReplyAction::decode(&bytes).unwrap_err();
    assert!(matches!(err, ActionPayloadDecodeError::Malformed { .. }));
}
