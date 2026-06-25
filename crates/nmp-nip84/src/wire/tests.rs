use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::{HighlightAttribution, HighlightSource, PublishHighlightInput};

use super::SCHEMA_VERSION;

#[test]
fn highlight_payload_round_trips_all_fields() {
    let action = PublishHighlightInput {
        highlighted_text: "quoted text".to_string(),
        context: Some("context".to_string()),
        comment: Some("comment".to_string()),
        source_refs: vec![
            HighlightSource::Event {
                event_id: "aa".repeat(32),
                relay: Some("wss://relay.example/".to_string()),
            },
            HighlightSource::External {
                external_id: "podcast:item:guid:episode-guid".to_string(),
                external_kind: "podcast:item:guid".to_string(),
                hint_url: Some("https://example.com/listen".to_string()),
            },
        ],
        attributions: vec![HighlightAttribution {
            pubkey: "bb".repeat(32),
            relay: None,
            role: Some("author".to_string()),
        }],
    };

    let decoded = PublishHighlightInput::decode(&action.encode()).expect("decode");
    assert_eq!(decoded, action);
}

#[test]
fn highlight_payload_rejects_wrong_identifier() {
    let err =
        PublishHighlightInput::decode(b"N25Rnot a highlight").expect_err("wrong file id rejected");
    assert!(matches!(err, ActionPayloadDecodeError::Malformed { .. }));
}

#[test]
fn highlight_payload_rejects_schema_version_mismatch() {
    let mut bytes = PublishHighlightInput {
        highlighted_text: "text".to_string(),
        ..Default::default()
    }
    .encode();
    let root_pos = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let table = root_pos;
    let vtable = table - u16::from_le_bytes(bytes[table..table + 2].try_into().unwrap()) as usize;
    let slot_offset =
        u16::from_le_bytes(bytes[vtable + 4..vtable + 6].try_into().unwrap()) as usize;
    let field = table + slot_offset;
    bytes[field..field + 4].copy_from_slice(&999_u32.to_le_bytes());

    let err = PublishHighlightInput::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}
