//! Typed FlatBuffers payload codec for `nmp.nip01.visible_note_relations`.

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
#[path = "wire/generated/visible_note_relations_generated.rs"]
pub mod visible_note_relations_generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};
use visible_note_relations_generated::nmp::relations as fb;

use crate::action::{VisibleNoteRelationsAction, VisibleNoteRelationsLifecycle};

pub const SCHEMA_VERSION: u32 = 1;

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

impl ActionPayload for VisibleNoteRelationsAction {
    const SCHEMA_ID: &'static str = "nmp.nip01.visible_note_relations";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let target_event_id = fbb.create_string(&self.target_event_id);
        let consumer_id = fbb.create_string(&self.consumer_id);
        let target_address = self
            .target_address
            .as_ref()
            .map(|address| fbb.create_string(address));
        let lifecycle = match self.lifecycle {
            VisibleNoteRelationsLifecycle::Claim => fb::VisibleNoteRelationsLifecycle::Claim,
            VisibleNoteRelationsLifecycle::Release => fb::VisibleNoteRelationsLifecycle::Release,
        };
        let payload = fb::VisibleNoteRelationsPayload::create(
            &mut fbb,
            &fb::VisibleNoteRelationsPayloadArgs {
                schema_version: SCHEMA_VERSION,
                lifecycle,
                target_event_id: Some(target_event_id),
                target_kind: self.target_kind,
                consumer_id: Some(consumer_id),
                target_address,
            },
        );
        fb::finish_visible_note_relations_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::visible_note_relations_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing VNRL file identifier"));
        }
        let root = fb::root_as_visible_note_relations_payload(bytes).map_err(|e| {
            malformed(format!(
                "not a valid VisibleNoteRelationsPayload buffer: {e}"
            ))
        })?;
        gate_schema_version(root.schema_version())?;
        let lifecycle = match root.lifecycle() {
            fb::VisibleNoteRelationsLifecycle::Claim => VisibleNoteRelationsLifecycle::Claim,
            fb::VisibleNoteRelationsLifecycle::Release => VisibleNoteRelationsLifecycle::Release,
            unknown => {
                return Err(malformed(format!(
                    "unknown VisibleNoteRelationsLifecycle ordinal: {}",
                    unknown.0
                )))
            }
        };
        Ok(VisibleNoteRelationsAction {
            lifecycle,
            target_event_id: root.target_event_id().to_string(),
            target_kind: root.target_kind(),
            consumer_id: root.consumer_id().to_string(),
            target_address: root.target_address().map(str::to_string),
        })
    }
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
