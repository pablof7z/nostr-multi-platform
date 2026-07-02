//! `ActionPayload` codec for `DiscoverGroupsInput` (`nmp.nip29.discover`).
//! (ADR-0071 / Cut-B producer gap #1756).

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::action::DiscoverGroupsInput;

use super::discover_groups_action_generated::nmp::nip_29 as discover_fb;
use super::{gate_schema_version, malformed, SCHEMA_VERSION};

// --- DiscoverGroupsInput -----------------------------------------------------

impl ActionPayload for DiscoverGroupsInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.discover";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let relay_url = fbb.create_string(&self.relay_url);
        let payload = discover_fb::DiscoverGroupsPayload::create(
            &mut fbb,
            &discover_fb::DiscoverGroupsPayloadArgs {
                schema_version: SCHEMA_VERSION,
                relay_url: Some(relay_url),
            },
        );
        discover_fb::finish_discover_groups_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !discover_fb::discover_groups_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N29D file identifier"));
        }
        let root = discover_fb::root_as_discover_groups_payload(bytes)
            .map_err(|e| malformed(format!("not a valid DiscoverGroupsPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        Ok(DiscoverGroupsInput {
            relay_url: root.relay_url().to_string(),
        })
    }
}
