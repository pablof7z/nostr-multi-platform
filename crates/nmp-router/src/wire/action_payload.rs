//! Typed FlatBuffers payload codec for the router-owned relay-list action
//! payload (ADR-0071 / #1756): `nmp.nip65.publish_relay_list`
//! ([`PublishRelayListInput`]).
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
#[path = "generated/publish_relay_list_generated.rs"]
pub mod publish_relay_list_generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use publish_relay_list_generated::nmp::router as relay_list_fb;

use crate::publish_relay_list::{PublishRelayListInput, RelayListEntry, RelayMarker};

/// Wire schema version for the router-owned relay-list action payload. Bump on
/// any breaking change to `publish_relay_list.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

// --- PublishRelayListInput (nmp.nip65.publish_relay_list) --------------------

/// Map the in-crate [`RelayMarker`] to its wire enum. Total — every variant has
/// a wire counterpart so encode never loses a marker.
fn marker_to_wire(marker: RelayMarker) -> relay_list_fb::RelayMarker {
    match marker {
        RelayMarker::Both => relay_list_fb::RelayMarker::Both,
        RelayMarker::Read => relay_list_fb::RelayMarker::Read,
        RelayMarker::Write => relay_list_fb::RelayMarker::Write,
        RelayMarker::Indexer => relay_list_fb::RelayMarker::Indexer,
    }
}

/// Map a wire enum value back to [`RelayMarker`]. An unknown ordinal (a
/// forward-compat value this build does not know) is rejected fail-closed
/// rather than silently coerced.
fn marker_from_wire(
    marker: relay_list_fb::RelayMarker,
) -> Result<RelayMarker, ActionPayloadDecodeError> {
    match marker {
        relay_list_fb::RelayMarker::Both => Ok(RelayMarker::Both),
        relay_list_fb::RelayMarker::Read => Ok(RelayMarker::Read),
        relay_list_fb::RelayMarker::Write => Ok(RelayMarker::Write),
        relay_list_fb::RelayMarker::Indexer => Ok(RelayMarker::Indexer),
        other => Err(malformed(format!(
            "unknown RelayMarker ordinal {}",
            other.0
        ))),
    }
}

impl ActionPayload for PublishRelayListInput {
    const SCHEMA_ID: &'static str = "nmp.nip65.publish_relay_list";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let entry_offsets: Vec<_> = self
            .relays
            .iter()
            .map(|entry| {
                let url = fbb.create_string(&entry.url);
                relay_list_fb::RelayListEntry::create(
                    &mut fbb,
                    &relay_list_fb::RelayListEntryArgs {
                        url: Some(url),
                        marker: marker_to_wire(entry.marker),
                    },
                )
            })
            .collect();
        let relays = fbb.create_vector(&entry_offsets);
        let payload = relay_list_fb::PublishRelayListPayload::create(
            &mut fbb,
            &relay_list_fb::PublishRelayListPayloadArgs {
                schema_version: SCHEMA_VERSION,
                relays: Some(relays),
            },
        );
        relay_list_fb::finish_publish_relay_list_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8
            || !relay_list_fb::publish_relay_list_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing N65P file identifier"));
        }
        let root = relay_list_fb::root_as_publish_relay_list_payload(bytes)
            .map_err(|e| malformed(format!("not a valid PublishRelayListPayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        let mut relays = Vec::new();
        if let Some(entries) = root.relays() {
            for entry in entries {
                relays.push(RelayListEntry {
                    url: entry.url().to_string(),
                    marker: marker_from_wire(entry.marker())?,
                });
            }
        }
        Ok(PublishRelayListInput { relays })
    }
}

#[cfg(test)]
#[path = "action_payload_tests.rs"]
mod tests;
