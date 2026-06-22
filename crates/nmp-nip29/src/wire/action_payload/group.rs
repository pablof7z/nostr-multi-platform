//! `ActionPayload` codecs for the core group-lifecycle / content actions:
//! `join`, `leave`, `post_chat_message`, `create_public_group`, and
//! `react_in_group` (ADR-0064 / S9 #1747).

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::action::{
    CreatePublicGroupInput, GroupAccess, GroupVisibility, JoinGroupInput, LeaveGroupInput,
    PostChatMessageInput, ReactInGroupInput,
};
use crate::group_id::GroupId;

use super::{gate_schema_version, malformed, SCHEMA_VERSION};

use super::create_public_group_action_generated::nmp::nip_29 as create_fb;
use super::join_group_action_generated::nmp::nip_29 as join_fb;
use super::leave_group_action_generated::nmp::nip_29 as leave_fb;
use super::post_chat_message_action_generated::nmp::nip_29 as chat_fb;
use super::react_in_group_action_generated::nmp::nip_29 as react_fb;

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

// --- PostChatMessageInput ----------------------------------------------------

impl ActionPayload for PostChatMessageInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.post_chat_message";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let host_relay_url = fbb.create_string(&self.group.host_relay_url);
        let local_id = fbb.create_string(&self.group.local_id);
        let group = chat_fb::GroupRef::create(
            &mut fbb,
            &chat_fb::GroupRefArgs {
                host_relay_url: Some(host_relay_url),
                local_id: Some(local_id),
            },
        );
        let content = fbb.create_string(&self.content);
        let prefix_offsets: Vec<_> = self
            .previous_event_id_prefixes
            .iter()
            .map(|p| fbb.create_string(p))
            .collect();
        let previous_event_id_prefixes = fbb.create_vector(&prefix_offsets);
        let reply_to_event_id = self
            .reply_to_event_id
            .as_ref()
            .map(|s| fbb.create_string(s));
        let payload = chat_fb::PostChatMessagePayload::create(
            &mut fbb,
            &chat_fb::PostChatMessagePayloadArgs {
                schema_version: SCHEMA_VERSION,
                group: Some(group),
                content: Some(content),
                previous_event_id_prefixes: Some(previous_event_id_prefixes),
                reply_to_event_id,
            },
        );
        chat_fb::finish_post_chat_message_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !chat_fb::post_chat_message_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N29C file identifier"));
        }
        let root = chat_fb::root_as_post_chat_message_payload(bytes)
            .map_err(|e| malformed(format!("not a valid PostChatMessagePayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let group = root.group();
        let previous_event_id_prefixes = root
            .previous_event_id_prefixes()
            .map(|v| v.iter().map(str::to_string).collect())
            .unwrap_or_default();
        Ok(PostChatMessageInput {
            group: GroupId::new(group.host_relay_url(), group.local_id()),
            content: root.content().to_string(),
            previous_event_id_prefixes,
            reply_to_event_id: root.reply_to_event_id().map(str::to_string),
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

// --- ReactInGroupInput -------------------------------------------------------

impl ActionPayload for ReactInGroupInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.react_in_group";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let host_relay_url = fbb.create_string(&self.group.host_relay_url);
        let local_id = fbb.create_string(&self.group.local_id);
        let group = react_fb::GroupRef::create(
            &mut fbb,
            &react_fb::GroupRefArgs {
                host_relay_url: Some(host_relay_url),
                local_id: Some(local_id),
            },
        );
        let target_event_id = fbb.create_string(&self.target_event_id);
        let target_author_pubkey = self
            .target_author_pubkey
            .as_ref()
            .map(|s| fbb.create_string(s));
        let content = fbb.create_string(&self.content);
        let payload = react_fb::ReactInGroupPayload::create(
            &mut fbb,
            &react_fb::ReactInGroupPayloadArgs {
                schema_version: SCHEMA_VERSION,
                group: Some(group),
                target_event_id: Some(target_event_id),
                target_author_pubkey,
                content: Some(content),
            },
        );
        react_fb::finish_react_in_group_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !react_fb::react_in_group_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N29X file identifier"));
        }
        let root = react_fb::root_as_react_in_group_payload(bytes)
            .map_err(|e| malformed(format!("not a valid ReactInGroupPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let group = root.group();
        Ok(ReactInGroupInput {
            group: GroupId::new(group.host_relay_url(), group.local_id()),
            target_event_id: root.target_event_id().to_string(),
            target_author_pubkey: root.target_author_pubkey().map(str::to_string),
            content: root.content().to_string(),
        })
    }
}
