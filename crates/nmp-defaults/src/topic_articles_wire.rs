//! Typed FlatBuffers payload codec for `nmp.app.topic_articles`.

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "topic_articles_wire/generated/topic_articles_generated.rs"]
mod topic_articles_generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};
use topic_articles_generated::nmp::defaults as topic_fb;

use crate::topic_articles::TopicArticlesAction;

const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

fn gate_schema_version(found: u32) -> Result<(), ActionPayloadDecodeError> {
    if found != SCHEMA_VERSION {
        return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
            found,
            expected: SCHEMA_VERSION,
        });
    }
    Ok(())
}

impl ActionPayload for TopicArticlesAction {
    const SCHEMA_ID: &'static str = "nmp.app.topic_articles";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let (op, topic_str, consumer_id_str) = match self {
            TopicArticlesAction::Claim { topic, consumer_id } => {
                (topic_fb::TopicArticlesOp::Claim, topic, consumer_id)
            }
            TopicArticlesAction::Release { topic, consumer_id } => {
                (topic_fb::TopicArticlesOp::Release, topic, consumer_id)
            }
        };
        let topic = fbb.create_string(topic_str);
        let consumer_id = fbb.create_string(consumer_id_str);
        let payload = topic_fb::TopicArticlesPayload::create(
            &mut fbb,
            &topic_fb::TopicArticlesPayloadArgs {
                schema_version: SCHEMA_VERSION,
                op,
                topic: Some(topic),
                consumer_id: Some(consumer_id),
            },
        );
        topic_fb::finish_topic_articles_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !topic_fb::topic_articles_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NTPC file identifier"));
        }
        let root = topic_fb::root_as_topic_articles_payload(bytes)
            .map_err(|e| malformed(format!("not a valid TopicArticlesPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let topic = root.topic().to_string();
        let consumer_id = root.consumer_id().to_string();
        match root.op() {
            topic_fb::TopicArticlesOp::Claim => {
                Ok(TopicArticlesAction::Claim { topic, consumer_id })
            }
            topic_fb::TopicArticlesOp::Release => {
                Ok(TopicArticlesAction::Release { topic, consumer_id })
            }
            unknown => Err(malformed(format!(
                "unknown TopicArticlesOp ordinal: {}",
                unknown.0
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch_schema_version(mut bytes: Vec<u8>, new_version: u32) -> Vec<u8> {
        let root_off = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let vtable_soff = i32::from_le_bytes([
            bytes[root_off],
            bytes[root_off + 1],
            bytes[root_off + 2],
            bytes[root_off + 3],
        ]);
        let vtable_off = (root_off as i64 - vtable_soff as i64) as usize;
        let field_off = u16::from_le_bytes([bytes[vtable_off + 4], bytes[vtable_off + 5]]) as usize;
        let abs = root_off + field_off;
        bytes[abs..abs + 4].copy_from_slice(&new_version.to_le_bytes());
        bytes
    }

    fn patch_op_ordinal(mut bytes: Vec<u8>, value: u8) -> Vec<u8> {
        let root_off = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let vtable_soff = i32::from_le_bytes([
            bytes[root_off],
            bytes[root_off + 1],
            bytes[root_off + 2],
            bytes[root_off + 3],
        ]);
        let vtable_off = (root_off as i64 - vtable_soff as i64) as usize;
        let field_off = u16::from_le_bytes([bytes[vtable_off + 6], bytes[vtable_off + 7]]) as usize;
        assert_ne!(field_off, 0, "Release op slot must be present");
        bytes[root_off + field_off] = value;
        bytes
    }

    #[test]
    fn claim_round_trips() {
        let action = TopicArticlesAction::Claim {
            topic: "nostr".to_string(),
            consumer_id: "discover-view".to_string(),
        };
        assert_eq!(
            TopicArticlesAction::decode(&action.encode()).expect("decodes"),
            action
        );
    }

    #[test]
    fn release_round_trips() {
        let action = TopicArticlesAction::Release {
            topic: "nostr".to_string(),
            consumer_id: "discover-view".to_string(),
        };
        assert_eq!(
            TopicArticlesAction::decode(&action.encode()).expect("decodes"),
            action
        );
    }

    #[test]
    fn malformed_buffer_is_rejected() {
        assert!(matches!(
            TopicArticlesAction::decode(b"junk"),
            Err(ActionPayloadDecodeError::Malformed { .. })
        ));
    }

    #[test]
    fn wrong_file_identifier_is_rejected() {
        let mut bytes = TopicArticlesAction::Release {
            topic: "nostr".to_string(),
            consumer_id: "discover-view".to_string(),
        }
        .encode();
        bytes[4..8].copy_from_slice(b"XXXX");
        assert!(matches!(
            TopicArticlesAction::decode(&bytes),
            Err(ActionPayloadDecodeError::Malformed { .. })
        ));
    }

    #[test]
    fn wrong_schema_version_is_rejected() {
        let bytes = TopicArticlesAction::Release {
            topic: "nostr".to_string(),
            consumer_id: "discover-view".to_string(),
        }
        .encode();
        let err = TopicArticlesAction::decode(&patch_schema_version(bytes, 999))
            .expect_err("schema mismatch must reject");
        assert_eq!(
            err,
            ActionPayloadDecodeError::SchemaVersionMismatch {
                found: 999,
                expected: 1
            }
        );
    }

    #[test]
    fn unknown_op_ordinal_is_rejected() {
        let bytes = TopicArticlesAction::Release {
            topic: "nostr".to_string(),
            consumer_id: "discover-view".to_string(),
        }
        .encode();
        assert!(matches!(
            TopicArticlesAction::decode(&patch_op_ordinal(bytes, 99)),
            Err(ActionPayloadDecodeError::Malformed { .. })
        ));
    }
}
