//! Tests for the `nmp.nip18.repost` action module: encode/decode round-trip and
//! kind:6 tag-parity with the retired `ChirpActionIntent::Repost` spec output.

use super::*;
use nmp_core::substrate::ActionContext;

const EVENT_ID: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const AUTHOR: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

#[test]
fn namespace_and_schema_id_match() {
    assert_eq!(RepostModule::NAMESPACE, "nmp.nip18.repost");
    assert_eq!(<RepostInput as ActionPayload>::SCHEMA_ID, "nmp.nip18.repost");
    assert_eq!(<RepostInput as ActionPayload>::SCHEMA_VERSION, 1);
}

#[test]
fn build_unsigned_emits_kind6_with_e_and_p_tags_and_empty_content() {
    let input = RepostInput {
        event_id: EVENT_ID.to_string(),
        author_pubkey: AUTHOR.to_string(),
    };
    let unsigned = RepostModule::build_unsigned(&input);
    assert_eq!(unsigned.kind, KIND_REPOST);
    assert_eq!(unsigned.content, "");
    assert_eq!(
        unsigned.tags,
        vec![
            vec!["e".to_string(), EVENT_ID.to_string()],
            vec!["p".to_string(), AUTHOR.to_string()],
        ]
    );
}

#[test]
fn start_rejects_non_hex_ids() {
    let bad = RepostInput {
        event_id: "short".to_string(),
        author_pubkey: AUTHOR.to_string(),
    };
    assert!(matches!(
        RepostModule.start(&mut ActionContext::default(), bad),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn encode_decode_round_trips() {
    let input = RepostInput {
        event_id: EVENT_ID.to_string(),
        author_pubkey: AUTHOR.to_string(),
    };
    let bytes = input.encode();
    assert_eq!(RepostInput::decode(&bytes).unwrap(), input);
}

#[test]
fn decode_rejects_wrong_schema_version() {
    use flatbuffers::FlatBufferBuilder;
    let mut fbb = FlatBufferBuilder::new();
    let event_id = fbb.create_string(EVENT_ID);
    let author = fbb.create_string(AUTHOR);
    let payload = repost_fb::RepostPayload::create(
        &mut fbb,
        &repost_fb::RepostPayloadArgs {
            schema_version: 999,
            event_id: Some(event_id),
            author_pubkey: Some(author),
        },
    );
    repost_fb::finish_repost_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    assert!(matches!(
        RepostInput::decode(&bytes),
        Err(ActionPayloadDecodeError::SchemaVersionMismatch { .. })
    ));
}
