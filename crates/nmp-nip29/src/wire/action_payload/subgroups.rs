//! `ActionPayload` codec for the `nmp.nip29.set_parent` action (ADR-0064 / S9
//! #1747) — adopt/detach a NIP-29 subgroup (nips PR #2319).

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::action::SetParentInput;
use crate::group_id::GroupId;

use super::{gate_schema_version, malformed, SCHEMA_VERSION};

use super::set_parent_action_generated::nmp::nip_29 as fb;

impl ActionPayload for SetParentInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.set_parent";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let host_relay_url = fbb.create_string(&self.group.host_relay_url);
        let local_id = fbb.create_string(&self.group.local_id);
        let group = fb::GroupRef::create(
            &mut fbb,
            &fb::GroupRefArgs {
                host_relay_url: Some(host_relay_url),
                local_id: Some(local_id),
            },
        );
        let parent = self.parent.as_ref().map(|s| fbb.create_string(s));
        let payload = fb::SetParentPayload::create(
            &mut fbb,
            &fb::SetParentPayloadArgs {
                schema_version: SCHEMA_VERSION,
                group: Some(group),
                parent,
            },
        );
        fb::finish_set_parent_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::set_parent_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N29S file identifier"));
        }
        let root = fb::root_as_set_parent_payload(bytes)
            .map_err(|e| malformed(format!("not a valid SetParentPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let group = root.group();
        Ok(SetParentInput {
            group: GroupId::new(group.host_relay_url(), group.local_id()),
            parent: root.parent().map(str::to_string),
        })
    }
}
