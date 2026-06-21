//! Typed FlatBuffers payload codecs for the nip02 follow-list ACTIONS
//! (ADR-0064 / S3 #1751): `nmp.follow` / `nmp.unfollow` ([`PubkeyAction`]) and
//! `nmp.follow_many` ([`FollowManyAction`]).
//!
//! These are the WRITE-direction typed payloads carried as the OPAQUE
//! `DispatchEnvelope.payload`. The registry adapter decodes them through
//! [`ActionPayload::decode`] here — the single typed-decode site — running the
//! fail-closed `schema_version` gate BEFORE `start()`. Distinct from
//! `typed_fb.rs`, which is the READ-direction projection sidecar.
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
#[path = "generated/follow_action_generated.rs"]
pub mod follow_action_generated;

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
#[path = "generated/follow_many_action_generated.rs"]
pub mod follow_many_action_generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use follow_action_generated::nmp::nip_02 as follow_fb;
use follow_many_action_generated::nmp::nip_02 as follow_many_fb;

use crate::{FollowManyAction, PubkeyAction};

/// Wire schema version for both nip02 follow-action payloads. Bump on any
/// breaking change to `follow_action.fbs` / `follow_many_action.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed { reason: reason.into() }
}

// --- PubkeyAction (nmp.follow / nmp.unfollow) --------------------------------

impl ActionPayload for PubkeyAction {
    const SCHEMA_ID: &'static str = "nmp.nip02.follow_action";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let pubkey = fbb.create_string(&self.pubkey);
        let payload = follow_fb::FollowActionPayload::create(
            &mut fbb,
            &follow_fb::FollowActionPayloadArgs {
                schema_version: SCHEMA_VERSION,
                pubkey: Some(pubkey),
            },
        );
        follow_fb::finish_follow_action_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !follow_fb::follow_action_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NF2A file identifier"));
        }
        let root = follow_fb::root_as_follow_action_payload(bytes)
            .map_err(|e| malformed(format!("not a valid FollowActionPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(PubkeyAction { pubkey: root.pubkey().to_string() })
    }
}

// --- FollowManyAction (nmp.follow_many) --------------------------------------

impl ActionPayload for FollowManyAction {
    const SCHEMA_ID: &'static str = "nmp.nip02.follow_many_action";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let pubkey_offsets: Vec<_> =
            self.pubkeys.iter().map(|p| fbb.create_string(p)).collect();
        let pubkeys = fbb.create_vector(&pubkey_offsets);
        let payload = follow_many_fb::FollowManyActionPayload::create(
            &mut fbb,
            &follow_many_fb::FollowManyActionPayloadArgs {
                schema_version: SCHEMA_VERSION,
                pubkeys: Some(pubkeys),
            },
        );
        follow_many_fb::finish_follow_many_action_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8
            || !follow_many_fb::follow_many_action_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing NFMA file identifier"));
        }
        let root = follow_many_fb::root_as_follow_many_action_payload(bytes)
            .map_err(|e| malformed(format!("not a valid FollowManyActionPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        let pubkeys = root
            .pubkeys()
            .map(|v| v.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        Ok(FollowManyAction { pubkeys })
    }
}

#[cfg(test)]
#[path = "action_payload_tests.rs"]
mod tests;
