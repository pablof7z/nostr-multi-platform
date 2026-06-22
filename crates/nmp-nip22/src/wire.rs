//! Typed FlatBuffers payload codec for the nip22 post-comment action
//! (ADR-0064 / S9 #1747): `nmp.nip22.post_comment` ([`PostCommentAction`]).
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
#[path = "wire/generated/post_comment_generated.rs"]
pub mod generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};
use generated::nmp::nip_22 as fb;

use crate::action::PostCommentAction;

/// Wire schema version for the nip22 post_comment payload. Bump on any
/// breaking change to `post_comment.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed { reason: reason.into() }
}

// --- PostCommentAction -------------------------------------------------------

impl ActionPayload for PostCommentAction {
    const SCHEMA_ID: &'static str = "nmp.nip22.post_comment";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let root_tag_name = fbb.create_string(&self.root_tag_name);
        let root_tag_value = fbb.create_string(&self.root_tag_value);
        let parent_event_id =
            self.parent_event_id.as_ref().map(|s| fbb.create_string(s));
        let root_author_pubkey =
            self.root_author_pubkey.as_ref().map(|s| fbb.create_string(s));
        let parent_author_pubkey =
            self.parent_author_pubkey.as_ref().map(|s| fbb.create_string(s));
        let content = fbb.create_string(&self.content);
        let payload = fb::PostComment::create(
            &mut fbb,
            &fb::PostCommentArgs {
                schema_version: SCHEMA_VERSION,
                root_tag_name: Some(root_tag_name),
                root_tag_value: Some(root_tag_value),
                root_kind: self.root_kind,
                parent_event_id,
                root_author_pubkey,
                parent_author_pubkey,
                content: Some(content),
            },
        );
        fb::finish_post_comment_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::post_comment_buffer_has_identifier(bytes) {
            return Err(malformed("missing N22C file identifier"));
        }
        let root = fb::root_as_post_comment(bytes)
            .map_err(|e| malformed(format!("not a valid PostComment buffer: {e}")))?;
        // Gate FIRST: schema_version check before any field extraction.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(PostCommentAction {
            root_tag_name: root.root_tag_name().to_string(),
            root_tag_value: root.root_tag_value().to_string(),
            root_kind: root.root_kind(),
            parent_event_id: root.parent_event_id().map(str::to_string),
            root_author_pubkey: root.root_author_pubkey().map(str::to_string),
            parent_author_pubkey: root.parent_author_pubkey().map(str::to_string),
            content: root.content().to_string(),
        })
    }
}

#[cfg(test)]
#[path = "wire/tests.rs"]
mod tests;
