//! Typed FlatBuffers payload codecs for NIP-18 write actions.

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
#[path = "wire/generated/repost_generated.rs"]
pub mod repost_generated;

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
#[path = "wire/generated/quote_repost_generated.rs"]
pub mod quote_repost_generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};
use quote_repost_generated::nmp::nip_18 as quote_fb;
use repost_generated::nmp::nip_18 as repost_fb;

use crate::action::{QuoteRepostAction, RepostAction};

pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

impl ActionPayload for RepostAction {
    const SCHEMA_ID: &'static str = "nmp.nip18.repost";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let target_event_id = fbb.create_string(&self.target_event_id);
        let target_author_pubkey = self
            .target_author_pubkey
            .as_ref()
            .map(|s| fbb.create_string(s));
        let relay_hint = self.relay_hint.as_ref().map(|s| fbb.create_string(s));
        let payload = repost_fb::RepostPayload::create(
            &mut fbb,
            &repost_fb::RepostPayloadArgs {
                schema_version: SCHEMA_VERSION,
                target_event_id: Some(target_event_id),
                target_kind: self.target_kind,
                target_author_pubkey,
                relay_hint,
            },
        );
        repost_fb::finish_repost_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !repost_fb::repost_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N18R file identifier"));
        }
        let root = repost_fb::root_as_repost_payload(bytes)
            .map_err(|e| malformed(format!("not a valid RepostPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(RepostAction {
            target_event_id: root.target_event_id().to_string(),
            target_kind: root.target_kind(),
            target_author_pubkey: root.target_author_pubkey().map(str::to_string),
            relay_hint: root.relay_hint().map(str::to_string),
        })
    }
}

impl ActionPayload for QuoteRepostAction {
    const SCHEMA_ID: &'static str = "nmp.nip18.quote_repost";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let target_event_id = fbb.create_string(&self.target_event_id);
        let target_author_pubkey = self
            .target_author_pubkey
            .as_ref()
            .map(|s| fbb.create_string(s));
        let relay_hint = self.relay_hint.as_ref().map(|s| fbb.create_string(s));
        let content = fbb.create_string(&self.content);
        let payload = quote_fb::QuoteRepostPayload::create(
            &mut fbb,
            &quote_fb::QuoteRepostPayloadArgs {
                schema_version: SCHEMA_VERSION,
                target_event_id: Some(target_event_id),
                target_kind: self.target_kind,
                target_author_pubkey,
                relay_hint,
                content: Some(content),
            },
        );
        quote_fb::finish_quote_repost_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !quote_fb::quote_repost_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N18Q file identifier"));
        }
        let root = quote_fb::root_as_quote_repost_payload(bytes)
            .map_err(|e| malformed(format!("not a valid QuoteRepostPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(QuoteRepostAction {
            target_event_id: root.target_event_id().to_string(),
            target_kind: root.target_kind(),
            target_author_pubkey: root.target_author_pubkey().map(str::to_string),
            relay_hint: root.relay_hint().map(str::to_string),
            content: root.content().to_string(),
        })
    }
}

#[cfg(test)]
#[path = "wire/tests.rs"]
mod tests;
