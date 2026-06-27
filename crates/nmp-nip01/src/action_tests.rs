//! Tests for the `nmp.nip01.publish_note` action module: encode/decode
//! round-trip and tag-parity with the retired `ChirpActionIntent::PublishNote`
//! spec output.

use super::*;
use nmp_core::substrate::ActionContext;

const PARENT_ID: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const AUTHOR: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const ROOT_ID: &str = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const MENTION: &str = "dd11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

fn tag_keys(unsigned: &UnsignedEvent) -> Vec<&str> {
    unsigned
        .tags
        .iter()
        .filter_map(|t| t.first())
        .map(String::as_str)
        .collect()
}

#[test]
fn namespace_and_schema_id_match() {
    assert_eq!(PublishNoteModule::NAMESPACE, "nmp.nip01.publish_note");
    assert_eq!(
        <PublishNoteInput as ActionPayload>::SCHEMA_ID,
        "nmp.nip01.publish_note"
    );
    assert_eq!(<PublishNoteInput as ActionPayload>::SCHEMA_VERSION, 2);
}

#[test]
fn root_note_has_no_tags() {
    let input = PublishNoteInput {
        content: "hello".to_string(),
        ..Default::default()
    };
    let unsigned = input.build_unsigned().unwrap();
    assert_eq!(unsigned.kind, KIND_SHORT_TEXT_NOTE);
    assert_eq!(unsigned.content, "hello");
    assert!(unsigned.tags.is_empty());
}

#[test]
fn minimal_reply_emits_root_and_reply_e_tags_on_same_id() {
    let input = PublishNoteInput {
        content: "reply".to_string(),
        reply_event_id: Some(PARENT_ID.to_string()),
        ..Default::default()
    };
    let unsigned = input.build_unsigned().unwrap();
    assert_eq!(tag_keys(&unsigned), vec!["e", "e"]);
    assert_eq!(unsigned.tags[0][1], PARENT_ID);
    assert_eq!(unsigned.tags[0][3], "root");
    assert_eq!(unsigned.tags[1][1], PARENT_ID);
    assert_eq!(unsigned.tags[1][3], "reply");
}

#[test]
fn full_reply_builds_nip10_root_reply_and_p_tags() {
    // Golden vector: exact byte-for-byte match against Note::reply_to(parent)
    // where parent has root_id, no relay, and one mentioned pubkey.
    let input = PublishNoteInput {
        content: "nested".to_string(),
        reply_event_id: Some(PARENT_ID.to_string()),
        reply_author_pubkey: Some(AUTHOR.to_string()),
        reply_root_event_id: Some(ROOT_ID.to_string()),
        reply_root_relay: None,
        reply_mentioned_pubkeys: vec![MENTION.to_string()],
    };
    let unsigned = input.build_unsigned().unwrap();
    // Exact tag vectors (NIP-10 marked form):
    //   ["e", ROOT_ID, "", "root"]   — root relay slot is empty (no relay hint)
    //   ["e", PARENT_ID, "", "reply"] — relay_hint is None on the builder
    //   ["p", AUTHOR]               — parent author (no relay on p-tags)
    //   ["p", MENTION]              — mentioned pubkey
    assert_eq!(unsigned.tags.len(), 4);
    assert_eq!(
        unsigned.tags[0],
        vec!["e", ROOT_ID, "", "root"],
        "root e-tag must point at ROOT_ID with empty relay and 'root' marker"
    );
    assert_eq!(
        unsigned.tags[1],
        vec!["e", PARENT_ID, "", "reply"],
        "reply e-tag must point at PARENT_ID with empty relay and 'reply' marker"
    );
    assert_eq!(
        unsigned.tags[2],
        vec!["p", AUTHOR],
        "parent-author p-tag must be first p row"
    );
    assert_eq!(
        unsigned.tags[3],
        vec!["p", MENTION],
        "mentioned-pubkey p-tag must follow parent-author"
    );
}

#[test]
fn full_reply_with_root_relay_hint_inherits_relay_on_root_e_tag() {
    // When reply_root_relay is set, the root e-tag must carry that relay hint.
    let input = PublishNoteInput {
        content: "nested".to_string(),
        reply_event_id: Some(PARENT_ID.to_string()),
        reply_author_pubkey: Some(AUTHOR.to_string()),
        reply_root_event_id: Some(ROOT_ID.to_string()),
        reply_root_relay: Some("wss://relay.example".to_string()),
        reply_mentioned_pubkeys: vec![MENTION.to_string()],
    };
    let unsigned = input.build_unsigned().unwrap();
    assert_eq!(unsigned.tags.len(), 4);
    assert_eq!(
        unsigned.tags[0],
        vec!["e", ROOT_ID, "wss://relay.example", "root"],
        "root e-tag must carry the relay hint from reply_root_relay"
    );
    assert_eq!(
        unsigned.tags[1],
        vec!["e", PARENT_ID, "", "reply"],
        "reply e-tag relay slot stays empty (no per-reply relay hint)"
    );
    assert_eq!(unsigned.tags[2], vec!["p", AUTHOR]);
    assert_eq!(unsigned.tags[3], vec!["p", MENTION]);
}

#[test]
fn full_reply_without_mentioned_pubkeys_emits_only_author_p_tag() {
    // reply_mentioned_pubkeys empty → only the parent-author p-tag, no more.
    let input = PublishNoteInput {
        content: "reply no mentions".to_string(),
        reply_event_id: Some(PARENT_ID.to_string()),
        reply_author_pubkey: Some(AUTHOR.to_string()),
        reply_root_event_id: Some(ROOT_ID.to_string()),
        reply_root_relay: None,
        reply_mentioned_pubkeys: vec![],
    };
    let unsigned = input.build_unsigned().unwrap();
    assert_eq!(tag_keys(&unsigned), vec!["e", "e", "p"]);
    assert_eq!(unsigned.tags[2], vec!["p", AUTHOR]);
}

#[test]
fn start_rejects_empty_content() {
    let input = PublishNoteInput {
        content: "   ".to_string(),
        ..Default::default()
    };
    assert!(matches!(
        PublishNoteModule.start(&mut ActionContext::default(), input),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn encode_decode_round_trips_every_field() {
    let input = PublishNoteInput {
        content: "body".to_string(),
        reply_event_id: Some(PARENT_ID.to_string()),
        reply_author_pubkey: Some(AUTHOR.to_string()),
        reply_root_event_id: Some(ROOT_ID.to_string()),
        reply_root_relay: Some("wss://r.example".to_string()),
        reply_mentioned_pubkeys: vec![MENTION.to_string()],
    };
    let bytes = input.encode();
    let decoded = PublishNoteInput::decode(&bytes).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn encode_decode_round_trips_a_root_note() {
    let input = PublishNoteInput {
        content: "just a note".to_string(),
        ..Default::default()
    };
    let bytes = input.encode();
    let decoded = PublishNoteInput::decode(&bytes).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn encode_decode_round_trips_minimal_reply() {
    // Only reply_event_id set — no author, no root, no mentions.
    let input = PublishNoteInput {
        content: "minimal reply".to_string(),
        reply_event_id: Some(PARENT_ID.to_string()),
        ..Default::default()
    };
    let bytes = input.encode();
    let decoded = PublishNoteInput::decode(&bytes).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn decode_rejects_wrong_schema_version() {
    use flatbuffers::FlatBufferBuilder;
    let mut fbb = FlatBufferBuilder::new();
    let content = fbb.create_string("x");
    let payload = note_fb::PublishNotePayload::create(
        &mut fbb,
        &note_fb::PublishNotePayloadArgs {
            schema_version: 999,
            content: Some(content),
            ..Default::default()
        },
    );
    note_fb::finish_publish_note_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    assert!(matches!(
        PublishNoteInput::decode(&bytes),
        Err(ActionPayloadDecodeError::SchemaVersionMismatch { .. })
    ));
}

#[test]
fn decode_rejects_old_schema_version_1() {
    // Buffers encoded with schema_version=1 (pre-removal of reply_created_at /
    // reply_content) must be rejected fail-closed — not silently misread with
    // wrong field alignment.
    use flatbuffers::FlatBufferBuilder;
    let mut fbb = FlatBufferBuilder::new();
    let content = fbb.create_string("old");
    let payload = note_fb::PublishNotePayload::create(
        &mut fbb,
        &note_fb::PublishNotePayloadArgs {
            schema_version: 1,
            content: Some(content),
            ..Default::default()
        },
    );
    note_fb::finish_publish_note_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    assert!(matches!(
        PublishNoteInput::decode(&bytes),
        Err(ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 1,
            expected: 2
        })
    ));
}
