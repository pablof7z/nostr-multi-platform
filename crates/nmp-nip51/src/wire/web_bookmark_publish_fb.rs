//! Typed FlatBuffers payload codec for the NIP-B0 web-bookmark publish action
//! (`nmp.nip51.publish_web_bookmark`).
//!
//! The codec preserves the raw input shape. URL normalization, optional metadata
//! trimming, and active-account authorization stay in the action module.

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
#[path = "generated/web_bookmark_publish_generated.rs"]
pub mod generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use generated::nmp::nip_51 as fb;

use crate::web_bookmarks::{PublishWebBookmarkInput, WebBookmarkDraft};

/// Wire schema version for the web-bookmark publish payload.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

impl ActionPayload for PublishWebBookmarkInput {
    const SCHEMA_ID: &'static str = "nmp.nip51.web_bookmark_publish";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let account_pubkey = fbb.create_string(&self.account_pubkey);
        let url = fbb.create_string(&self.bookmark.url);
        let title = self
            .bookmark
            .title
            .as_deref()
            .map(|title| fbb.create_string(title));
        let description = self
            .bookmark
            .description
            .as_deref()
            .map(|description| fbb.create_string(description));
        let hashtag_offsets: Vec<_> = self
            .bookmark
            .hashtags
            .iter()
            .map(|hashtag| fbb.create_string(hashtag))
            .collect();
        let hashtags = (!hashtag_offsets.is_empty()).then(|| fbb.create_vector(&hashtag_offsets));
        let payload = fb::WebBookmarkPublishPayload::create(
            &mut fbb,
            &fb::WebBookmarkPublishPayloadArgs {
                schema_version: SCHEMA_VERSION,
                account_pubkey: Some(account_pubkey),
                url: Some(url),
                title,
                description,
                published_at: self.bookmark.published_at.unwrap_or_default(),
                has_published_at: self.bookmark.published_at.is_some(),
                hashtags,
            },
        );
        fb::finish_web_bookmark_publish_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::web_bookmark_publish_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N51W file identifier"));
        }
        let root = fb::root_as_web_bookmark_publish_payload(bytes)
            .map_err(|e| malformed(format!("not a valid WebBookmarkPublishPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        let hashtags = root
            .hashtags()
            .map(|values| values.iter().map(str::to_string).collect())
            .unwrap_or_default();
        Ok(PublishWebBookmarkInput {
            account_pubkey: root.account_pubkey().to_string(),
            bookmark: WebBookmarkDraft {
                url: root.url().to_string(),
                title: root.title().map(str::to_string),
                description: root.description().map(str::to_string),
                published_at: root.has_published_at().then(|| root.published_at()),
                hashtags,
            },
        })
    }
}

#[cfg(test)]
#[path = "web_bookmark_publish_fb_tests.rs"]
mod tests;
