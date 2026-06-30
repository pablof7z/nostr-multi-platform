//! `ActionPayload` codecs for the core group-lifecycle / content actions:
//! `join`, `leave`, `publish_group_event`, and `create_public_group`
//! (ADR-0064 / S9 #1747).

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::action::{
    CreatePublicGroupInput, GroupAccess, GroupVisibility, JoinGroupInput, LeaveGroupInput,
    PublishGroupEventInput,
};
use crate::group_id::GroupId;

use super::{gate_schema_version, malformed, SCHEMA_VERSION};

use super::create_public_group_action_generated::nmp::nip_29 as create_fb;
use super::join_group_action_generated::nmp::nip_29 as join_fb;
use super::leave_group_action_generated::nmp::nip_29 as leave_fb;
use super::publish_group_event_action_generated::nmp::nip_29 as publish_fb;

// --- JoinGroupInput ----------------------------------------------------------

impl ActionPayload for JoinGroupInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.join";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let group = encode_join_group_ref(&mut fbb, &self.group);
        let invite_code = self.invite_code.as_ref().map(|s| fbb.create_string(s));
        let reason = self.reason.as_ref().map(|s| fbb.create_string(s));
        let payload = join_fb::JoinGroupPayload::create(
            &mut fbb,
            &join_fb::JoinGroupPayloadArgs {
                schema_version: SCHEMA_VERSION,
                group: Some(group),
                invite_code,
                reason,
            },
        );
        join_fb::finish_join_group_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !join_fb::join_group_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N29J file identifier"));
        }
        let root = join_fb::root_as_join_group_payload(bytes)
            .map_err(|e| malformed(format!("not a valid JoinGroupPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        Ok(JoinGroupInput {
            group: decode_join_group_ref(root.group()),
            invite_code: root.invite_code().map(str::to_string),
            reason: root.reason().map(str::to_string),
        })
    }
}

fn encode_join_group_ref<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    group: &GroupId,
) -> flatbuffers::WIPOffset<join_fb::GroupRef<'a>> {
    let host_relay_url = fbb.create_string(&group.host_relay_url);
    let local_id = fbb.create_string(&group.local_id);
    join_fb::GroupRef::create(
        fbb,
        &join_fb::GroupRefArgs {
            host_relay_url: Some(host_relay_url),
            local_id: Some(local_id),
        },
    )
}

fn decode_join_group_ref(group: join_fb::GroupRef<'_>) -> GroupId {
    GroupId::new(group.host_relay_url(), group.local_id())
}

// --- LeaveGroupInput ---------------------------------------------------------

impl ActionPayload for LeaveGroupInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.leave";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let host_relay_url = fbb.create_string(&self.group.host_relay_url);
        let local_id = fbb.create_string(&self.group.local_id);
        let group = leave_fb::GroupRef::create(
            &mut fbb,
            &leave_fb::GroupRefArgs {
                host_relay_url: Some(host_relay_url),
                local_id: Some(local_id),
            },
        );
        let reason = self.reason.as_ref().map(|s| fbb.create_string(s));
        let payload = leave_fb::LeaveGroupPayload::create(
            &mut fbb,
            &leave_fb::LeaveGroupPayloadArgs {
                schema_version: SCHEMA_VERSION,
                group: Some(group),
                reason,
            },
        );
        leave_fb::finish_leave_group_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !leave_fb::leave_group_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N29L file identifier"));
        }
        let root = leave_fb::root_as_leave_group_payload(bytes)
            .map_err(|e| malformed(format!("not a valid LeaveGroupPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let group = root.group();
        Ok(LeaveGroupInput {
            group: GroupId::new(group.host_relay_url(), group.local_id()),
            reason: root.reason().map(str::to_string),
        })
    }
}

// --- PublishGroupEventInput --------------------------------------------------

impl ActionPayload for PublishGroupEventInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.publish_group_event";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let host_relay_url = fbb.create_string(&self.group.host_relay_url);
        let local_id = fbb.create_string(&self.group.local_id);
        let group = publish_fb::GroupRef::create(
            &mut fbb,
            &publish_fb::GroupRefArgs {
                host_relay_url: Some(host_relay_url),
                local_id: Some(local_id),
            },
        );
        let content = fbb.create_string(&self.content);
        let tag_offsets: Vec<_> = self
            .tags
            .iter()
            .map(|tag| {
                let value_offsets: Vec<_> = tag.iter().map(|v| fbb.create_string(v)).collect();
                let values = fbb.create_vector(&value_offsets);
                publish_fb::StringTag::create(
                    &mut fbb,
                    &publish_fb::StringTagArgs {
                        values: Some(values),
                    },
                )
            })
            .collect();
        let tags = fbb.create_vector(&tag_offsets);
        let payload = publish_fb::PublishGroupEventPayload::create(
            &mut fbb,
            &publish_fb::PublishGroupEventPayloadArgs {
                schema_version: SCHEMA_VERSION,
                group: Some(group),
                kind: self.kind,
                content: Some(content),
                tags: Some(tags),
            },
        );
        publish_fb::finish_publish_group_event_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !publish_fb::publish_group_event_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing N29G file identifier"));
        }
        let root = publish_fb::root_as_publish_group_event_payload(bytes)
            .map_err(|e| malformed(format!("not a valid PublishGroupEventPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let group = root.group();
        let tags = root
            .tags()
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        row.values()
                            .map(|vs| vs.iter().map(str::to_string).collect())
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(PublishGroupEventInput {
            group: GroupId::new(group.host_relay_url(), group.local_id()),
            kind: root.kind(),
            content: root.content().map(str::to_string).unwrap_or_default(),
            tags,
        })
    }
}

// --- CreatePublicGroupInput --------------------------------------------------

impl ActionPayload for CreatePublicGroupInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.create_public_group";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let host_relay_url = fbb.create_string(&self.group.host_relay_url);
        let local_id = fbb.create_string(&self.group.local_id);
        let group = create_fb::GroupRef::create(
            &mut fbb,
            &create_fb::GroupRefArgs {
                host_relay_url: Some(host_relay_url),
                local_id: Some(local_id),
            },
        );
        let name = fbb.create_string(&self.name);
        let about = self.about.as_ref().map(|s| fbb.create_string(s));
        let picture = self.picture.as_ref().map(|s| fbb.create_string(s));
        // NIP-29 subgroups (#2319): optional parent local id on create.
        let parent = self.parent.as_ref().map(|s| fbb.create_string(s));
        let payload = create_fb::CreatePublicGroupPayload::create(
            &mut fbb,
            &create_fb::CreatePublicGroupPayloadArgs {
                schema_version: SCHEMA_VERSION,
                group: Some(group),
                name: Some(name),
                about,
                picture,
                visibility: encode_visibility(&self.visibility),
                access: encode_access(&self.access),
                parent,
            },
        );
        create_fb::finish_create_public_group_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !create_fb::create_public_group_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N29P file identifier"));
        }
        let root = create_fb::root_as_create_public_group_payload(bytes)
            .map_err(|e| malformed(format!("not a valid CreatePublicGroupPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let group = root.group();
        Ok(CreatePublicGroupInput {
            group: GroupId::new(group.host_relay_url(), group.local_id()),
            name: root.name().to_string(),
            about: root.about().map(str::to_string),
            picture: root.picture().map(str::to_string),
            visibility: decode_visibility(root.visibility()),
            access: decode_access(root.access()),
            parent: root.parent().map(str::to_string),
        })
    }
}

fn encode_visibility(v: &GroupVisibility) -> create_fb::GroupVisibility {
    match v {
        GroupVisibility::Public => create_fb::GroupVisibility::Public,
        GroupVisibility::Private => create_fb::GroupVisibility::Private,
    }
}

fn decode_visibility(v: create_fb::GroupVisibility) -> GroupVisibility {
    match v {
        create_fb::GroupVisibility::Private => GroupVisibility::Private,
        // Default / unknown enum value decodes to Public (the schema default).
        _ => GroupVisibility::Public,
    }
}

fn encode_access(a: &GroupAccess) -> create_fb::GroupAccess {
    match a {
        GroupAccess::Open => create_fb::GroupAccess::Open,
        GroupAccess::Closed => create_fb::GroupAccess::Closed,
    }
}

fn decode_access(a: create_fb::GroupAccess) -> GroupAccess {
    match a {
        create_fb::GroupAccess::Closed => GroupAccess::Closed,
        // Default / unknown enum value decodes to Open (the schema default).
        _ => GroupAccess::Open,
    }
}
