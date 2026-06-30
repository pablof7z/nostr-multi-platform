//! Typed FlatBuffers payload codec for the `nmp.replies.reply` action.

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
#[path = "wire/generated/reply_generated.rs"]
pub mod generated;

use generated::nmp::replies as fb;
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::action::ReplyAction;

pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

impl ActionPayload for ReplyAction {
    const SCHEMA_ID: &'static str = "nmp.replies.reply";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let target_event_id = self.target_event_id.as_ref().map(|s| fbb.create_string(s));
        let target_author_pubkey = self
            .target_author_pubkey
            .as_ref()
            .map(|s| fbb.create_string(s));
        let target_address = self.target_address.as_ref().map(|s| fbb.create_string(s));
        let target_external_uri = self
            .target_external_uri
            .as_ref()
            .map(|s| fbb.create_string(s));
        let relay_hint = self.relay_hint.as_ref().map(|s| fbb.create_string(s));
        let content = fbb.create_string(&self.content);
        let payload = fb::ReplyPayload::create(
            &mut fbb,
            &fb::ReplyPayloadArgs {
                schema_version: SCHEMA_VERSION,
                target_event_id,
                target_kind: self.target_kind,
                target_author_pubkey,
                target_address,
                target_external_uri,
                relay_hint,
                content: Some(content),
            },
        );
        fb::finish_reply_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::reply_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NRPY file identifier"));
        }
        let root = fb::root_as_reply_payload(bytes)
            .map_err(|e| malformed(format!("not a valid ReplyPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(ReplyAction {
            target_event_id: root.target_event_id().map(str::to_string),
            target_kind: root.target_kind(),
            target_author_pubkey: root.target_author_pubkey().map(str::to_string),
            target_address: root.target_address().map(str::to_string),
            target_external_uri: root.target_external_uri().map(str::to_string),
            relay_hint: root.relay_hint().map(str::to_string),
            content: root.content().to_string(),
        })
    }
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
