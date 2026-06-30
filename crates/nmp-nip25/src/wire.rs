//! Typed FlatBuffers payload codecs for the nip25 reaction actions
//! (ADR-0064 / S3 #1751): `nmp.nip25.react` ([`ReactAction`]) and
//! `nmp.nip25.unreact` ([`UnreactAction`]).
//!
//! These are the WRITE-direction typed payloads carried as the OPAQUE
//! `DispatchEnvelope.payload`. The registry adapter decodes them through
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
#[path = "wire/generated/react_generated.rs"]
pub mod react_generated;

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
#[path = "wire/generated/unreact_generated.rs"]
pub mod unreact_generated;

// Typed FlatBuffers codec for the NIP-25 reaction-aggregate READ projection
// (the `nmp.nip25.reactions` typed sidecar). Distinct from the write-direction
// react/unreact action payloads above.
pub mod reaction_aggregate_fb;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};
use react_generated::nmp::nip_25 as react_fb;
use unreact_generated::nmp::nip_25 as unreact_fb;

use crate::action::{ReactAction, UnreactAction};

/// Wire schema version for both nip25 reaction payloads. Bump on any breaking
/// change to `react.fbs` / `unreact.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

// --- ReactAction -------------------------------------------------------------

impl ActionPayload for ReactAction {
    const SCHEMA_ID: &'static str = "nmp.nip25.react";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let target_event_id = fbb.create_string(&self.target_event_id);
        let reaction = fbb.create_string(&self.reaction);
        let target_author_pubkey = self
            .target_author_pubkey
            .as_ref()
            .map(|s| fbb.create_string(s));
        let payload = react_fb::ReactPayload::create(
            &mut fbb,
            &react_fb::ReactPayloadArgs {
                schema_version: SCHEMA_VERSION,
                target_event_id: Some(target_event_id),
                reaction: Some(reaction),
                target_author_pubkey,
            },
        );
        react_fb::finish_react_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !react_fb::react_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N25R file identifier"));
        }
        let root = react_fb::root_as_react_payload(bytes)
            .map_err(|e| malformed(format!("not a valid ReactPayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(ReactAction {
            target_event_id: root.target_event_id().to_string(),
            reaction: root.reaction().to_string(),
            target_author_pubkey: root.target_author_pubkey().map(str::to_string),
        })
    }
}

// --- UnreactAction -----------------------------------------------------------

impl ActionPayload for UnreactAction {
    const SCHEMA_ID: &'static str = "nmp.nip25.unreact";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let reaction_event_id = fbb.create_string(&self.reaction_event_id);
        let reason = fbb.create_string(&self.reason);
        let payload = unreact_fb::UnreactPayload::create(
            &mut fbb,
            &unreact_fb::UnreactPayloadArgs {
                schema_version: SCHEMA_VERSION,
                reaction_event_id: Some(reaction_event_id),
                reason: Some(reason),
            },
        );
        unreact_fb::finish_unreact_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !unreact_fb::unreact_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N25U file identifier"));
        }
        let root = unreact_fb::root_as_unreact_payload(bytes)
            .map_err(|e| malformed(format!("not a valid UnreactPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(UnreactAction {
            reaction_event_id: root.reaction_event_id().to_string(),
            reason: root.reason().to_string(),
        })
    }
}

#[cfg(test)]
#[path = "wire/tests.rs"]
mod tests;
