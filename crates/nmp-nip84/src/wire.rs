//! Typed FlatBuffers payload codec for the NIP-84 highlight action
//! (#1649): `nmp.nip84.publish_highlight` ([`PublishHighlightAction`]).
//!
//! This is the WRITE-direction typed payload carried as the OPAQUE
//! `DispatchEnvelope.payload`. The registry adapter decodes it through
//! [`ActionPayload::decode`] here — the single typed-decode site — running the
//! fail-closed `schema_version` gate BEFORE `start()`.
//!
//! Honours D6: decode returns a data-shaped [`ActionPayloadDecodeError`] on any
//! malformed input; no panics on the decode path.

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
#[path = "wire/generated/publish_highlight_generated.rs"]
pub mod publish_highlight_generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};
use publish_highlight_generated::nmp::nip_84 as highlight_fb;

use crate::action::PublishHighlightAction;

/// Wire schema version for the highlight payload. Bump on any breaking change to
/// `publish_highlight.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value.filter(|s| !s.is_empty()).map(str::to_string)
}

impl ActionPayload for PublishHighlightAction {
    const SCHEMA_ID: &'static str = "nmp.nip84.publish_highlight";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let content = fbb.create_string(&self.content);
        let context = self.context.as_ref().map(|s| fbb.create_string(s));
        let source_event_id = self.source_event_id.as_ref().map(|s| fbb.create_string(s));
        let source_address = self.source_address.as_ref().map(|s| fbb.create_string(s));
        let source_author_pubkey = self
            .source_author_pubkey
            .as_ref()
            .map(|s| fbb.create_string(s));
        let alt = self.alt.as_ref().map(|s| fbb.create_string(s));
        let external_ids = if self.external_ids.is_empty() {
            None
        } else {
            let offsets: Vec<_> = self
                .external_ids
                .iter()
                .map(|s| fbb.create_string(s))
                .collect();
            Some(fbb.create_vector(&offsets))
        };
        let payload = highlight_fb::PublishHighlightPayload::create(
            &mut fbb,
            &highlight_fb::PublishHighlightPayloadArgs {
                schema_version: SCHEMA_VERSION,
                content: Some(content),
                context,
                source_event_id,
                source_address,
                source_author_pubkey,
                alt,
                external_ids,
            },
        );
        highlight_fb::finish_publish_highlight_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !highlight_fb::publish_highlight_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing N84H file identifier"));
        }
        let root = highlight_fb::root_as_publish_highlight_payload(bytes)
            .map_err(|e| malformed(format!("not a valid PublishHighlightPayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        let external_ids = root
            .external_ids()
            .map(|v| v.iter().map(str::to_string).collect())
            .unwrap_or_default();
        Ok(PublishHighlightAction {
            content: root.content().to_string(),
            context: non_empty(root.context()),
            source_event_id: non_empty(root.source_event_id()),
            source_address: non_empty(root.source_address()),
            source_author_pubkey: non_empty(root.source_author_pubkey()),
            alt: non_empty(root.alt()),
            external_ids,
        })
    }
}

#[cfg(test)]
#[path = "wire/tests.rs"]
mod tests;
