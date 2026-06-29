//! `ActionPayload` codec for the `nmp.nip29.edit_metadata` action (ADR-0064 /
//! S9 #1747) — edit an existing NIP-29 group's metadata (kind:9002).

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::action::{EditMetadataInput, GroupAccess, GroupVisibility};
use crate::group_id::GroupId;

use super::{gate_schema_version, malformed, SCHEMA_VERSION};

use super::edit_metadata_action_generated::nmp::nip_29 as fb;

impl ActionPayload for EditMetadataInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.edit_metadata";
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
        let name = self.name.as_ref().map(|s| fbb.create_string(s));
        let about = self.about.as_ref().map(|s| fbb.create_string(s));
        let picture = self.picture.as_ref().map(|s| fbb.create_string(s));
        let payload = fb::EditMetadataPayload::create(
            &mut fbb,
            &fb::EditMetadataPayloadArgs {
                schema_version: SCHEMA_VERSION,
                group: Some(group),
                name,
                about,
                picture,
                visibility: encode_visibility(self.visibility),
                access: encode_access(self.access),
            },
        );
        fb::finish_edit_metadata_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::edit_metadata_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N29E file identifier"));
        }
        let root = fb::root_as_edit_metadata_payload(bytes)
            .map_err(|e| malformed(format!("not a valid EditMetadataPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let group = root.group();
        Ok(EditMetadataInput {
            group: GroupId::new(group.host_relay_url(), group.local_id()),
            name: root.name().map(str::to_string),
            about: root.about().map(str::to_string),
            picture: root.picture().map(str::to_string),
            visibility: decode_visibility(root.visibility()),
            access: decode_access(root.access()),
        })
    }
}

fn encode_visibility(v: Option<GroupVisibility>) -> fb::EditVisibility {
    match v {
        None => fb::EditVisibility::Unset,
        Some(GroupVisibility::Public) => fb::EditVisibility::Public,
        Some(GroupVisibility::Private) => fb::EditVisibility::Private,
    }
}

fn decode_visibility(v: fb::EditVisibility) -> Option<GroupVisibility> {
    match v {
        fb::EditVisibility::Public => Some(GroupVisibility::Public),
        fb::EditVisibility::Private => Some(GroupVisibility::Private),
        // Unset (or any unknown discriminant) → leave prior value.
        _ => None,
    }
}

fn encode_access(a: Option<GroupAccess>) -> fb::EditAccess {
    match a {
        None => fb::EditAccess::Unset,
        Some(GroupAccess::Open) => fb::EditAccess::Open,
        Some(GroupAccess::Closed) => fb::EditAccess::Closed,
    }
}

fn decode_access(a: fb::EditAccess) -> Option<GroupAccess> {
    match a {
        fb::EditAccess::Open => Some(GroupAccess::Open),
        fb::EditAccess::Closed => Some(GroupAccess::Closed),
        _ => None,
    }
}
