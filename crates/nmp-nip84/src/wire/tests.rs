use super::*;
use crate::action::PublishHighlightAction;

fn sample() -> PublishHighlightAction {
    PublishHighlightAction {
        content: "highlighted".to_string(),
        context: Some("ctx".to_string()),
        source_event_id: Some(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        ),
        source_address: Some("30023:abc:slug".to_string()),
        source_author_pubkey: None,
        alt: None,
        external_ids: vec![
            "https://example.com".to_string(),
            "podcast:item:guid:9".to_string(),
        ],
    }
}

#[test]
fn encode_decode_round_trips() {
    let action = sample();
    let bytes = action.encode();
    let decoded = PublishHighlightAction::decode(&bytes).expect("decode succeeds");
    assert_eq!(decoded, action);
}

#[test]
fn encode_decode_minimal_round_trips() {
    let action = PublishHighlightAction {
        content: "only text".to_string(),
        context: None,
        source_event_id: None,
        source_address: None,
        source_author_pubkey: None,
        alt: None,
        external_ids: Vec::new(),
    };
    let bytes = action.encode();
    let decoded = PublishHighlightAction::decode(&bytes).expect("decode succeeds");
    assert_eq!(decoded, action);
}

#[test]
fn decode_rejects_schema_version_mismatch() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let content = fbb.create_string("x");
    let payload = highlight_fb::PublishHighlightPayload::create(
        &mut fbb,
        &highlight_fb::PublishHighlightPayloadArgs {
            schema_version: SCHEMA_VERSION + 1,
            content: Some(content),
            ..Default::default()
        },
    );
    highlight_fb::finish_publish_highlight_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    assert!(matches!(
        PublishHighlightAction::decode(&bytes),
        Err(ActionPayloadDecodeError::SchemaVersionMismatch { expected: 2, .. })
    ));
}

#[test]
fn decode_rejects_garbage_bytes() {
    assert!(matches!(
        PublishHighlightAction::decode(b"not a flatbuffer"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
