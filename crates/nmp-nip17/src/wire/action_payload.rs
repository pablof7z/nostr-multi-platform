//! Typed FlatBuffers payload codecs for the nip17 DM action payloads
//! (ADR-0071 / S9 #1747): `nmp.nip17.send` ([`SendDmInput`]) and
//! `nmp.nip17.publish_relay_list` ([`PublishDmRelayListInput`]).
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
#[path = "generated/send_generated.rs"]
pub mod send_generated;

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
#[path = "generated/dm_relay_list_action_generated.rs"]
pub mod dm_relay_list_action_generated;

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
#[path = "generated/hydrate_peer_relay_list_generated.rs"]
pub mod hydrate_peer_relay_list_generated;

use dm_relay_list_action_generated::nmp::nip_17 as relay_list_fb;
use hydrate_peer_relay_list_generated::nmp::nip_17 as hydrate_fb;
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};
use send_generated::nmp::nip_17 as send_fb;

use crate::action::{HydratePeerRelayListInput, SendDmInput};
use crate::dm_relay_list::PublishDmRelayListInput;

/// Wire schema version for both nip17 action payloads. Bump on any breaking
/// change to `send.fbs` / `dm_relay_list_action.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

// --- HydratePeerRelayListInput (nmp.nip17.hydrate_peer_relay_list) -----------

impl ActionPayload for HydratePeerRelayListInput {
    const SCHEMA_ID: &'static str = "nmp.nip17.hydrate_peer_relay_list";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let peer_pubkey = fbb.create_string(&self.peer_pubkey);
        let payload = hydrate_fb::HydratePeerRelayListPayload::create(
            &mut fbb,
            &hydrate_fb::HydratePeerRelayListPayloadArgs {
                schema_version: SCHEMA_VERSION,
                peer_pubkey: Some(peer_pubkey),
            },
        );
        hydrate_fb::finish_hydrate_peer_relay_list_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8
            || !hydrate_fb::hydrate_peer_relay_list_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing N17H file identifier"));
        }
        let root = hydrate_fb::root_as_hydrate_peer_relay_list_payload(bytes).map_err(|e| {
            malformed(format!(
                "not a valid HydratePeerRelayListPayload buffer: {e}"
            ))
        })?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(HydratePeerRelayListInput {
            peer_pubkey: root.peer_pubkey().to_string(),
        })
    }
}

// --- SendDmInput (nmp.nip17.send) --------------------------------------------

impl ActionPayload for SendDmInput {
    const SCHEMA_ID: &'static str = "nmp.nip17.send";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let recipient_pubkey = fbb.create_string(&self.recipient_pubkey);
        let content = fbb.create_string(&self.content);
        let reply_to = self.reply_to.as_ref().map(|s| fbb.create_string(s));
        let payload = send_fb::SendDmPayload::create(
            &mut fbb,
            &send_fb::SendDmPayloadArgs {
                schema_version: SCHEMA_VERSION,
                recipient_pubkey: Some(recipient_pubkey),
                content: Some(content),
                reply_to,
            },
        );
        send_fb::finish_send_dm_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !send_fb::send_dm_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N17S file identifier"));
        }
        let root = send_fb::root_as_send_dm_payload(bytes)
            .map_err(|e| malformed(format!("not a valid SendDmPayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(SendDmInput {
            recipient_pubkey: root.recipient_pubkey().to_string(),
            content: root.content().to_string(),
            reply_to: root.reply_to().map(str::to_string),
        })
    }
}

// --- PublishDmRelayListInput (nmp.nip17.publish_relay_list) ------------------

impl ActionPayload for PublishDmRelayListInput {
    const SCHEMA_ID: &'static str = "nmp.nip17.publish_relay_list";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let relay_offsets: Vec<_> = self.relays.iter().map(|r| fbb.create_string(r)).collect();
        let relays = fbb.create_vector(&relay_offsets);
        let payload = relay_list_fb::PublishDmRelayListPayload::create(
            &mut fbb,
            &relay_list_fb::PublishDmRelayListPayloadArgs {
                schema_version: SCHEMA_VERSION,
                relays: Some(relays),
            },
        );
        relay_list_fb::finish_publish_dm_relay_list_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8
            || !relay_list_fb::publish_dm_relay_list_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing N17R file identifier"));
        }
        let root = relay_list_fb::root_as_publish_dm_relay_list_payload(bytes)
            .map_err(|e| malformed(format!("not a valid PublishDmRelayListPayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        let relays = root
            .relays()
            .map(|v| v.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        Ok(PublishDmRelayListInput { relays })
    }
}

#[cfg(test)]
#[path = "action_payload_tests.rs"]
mod tests;
