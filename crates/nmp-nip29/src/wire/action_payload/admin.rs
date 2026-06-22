//! `ActionPayload` codecs for the ADR-0060 admin actions: `put_user`
//! (kind:9000) and `create_invite` (kind:9009) (ADR-0064 / S9 #1747).

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::action::{CreateInviteInput, PutUserInput};
use crate::group_id::GroupId;

use super::{gate_schema_version, malformed, SCHEMA_VERSION};

use super::create_invite_action_generated::nmp::nip_29 as invite_fb;
use super::put_user_action_generated::nmp::nip_29 as put_user_fb;

// --- PutUserInput ------------------------------------------------------------

impl ActionPayload for PutUserInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.put_user";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let host_relay_url = fbb.create_string(&self.group.host_relay_url);
        let local_id = fbb.create_string(&self.group.local_id);
        let group = put_user_fb::GroupRef::create(
            &mut fbb,
            &put_user_fb::GroupRefArgs {
                host_relay_url: Some(host_relay_url),
                local_id: Some(local_id),
            },
        );
        let target_pubkey = fbb.create_string(&self.target_pubkey);
        // PRESENCE-CRITICAL: `start()` rejects a present-but-empty role
        // (`Some("")` -> Invalid). Encode the option faithfully so the decode
        // side can reproduce `Some("")` and NOT collapse it to `None` (which
        // would bypass that check — fail-open).
        let role = self.role.as_ref().map(|s| fbb.create_string(s));
        let reason = self.reason.as_ref().map(|s| fbb.create_string(s));
        let payload = put_user_fb::PutUserPayload::create(
            &mut fbb,
            &put_user_fb::PutUserPayloadArgs {
                schema_version: SCHEMA_VERSION,
                group: Some(group),
                target_pubkey: Some(target_pubkey),
                role,
                reason,
            },
        );
        put_user_fb::finish_put_user_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !put_user_fb::put_user_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N29U file identifier"));
        }
        let root = put_user_fb::root_as_put_user_payload(bytes)
            .map_err(|e| malformed(format!("not a valid PutUserPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let group = root.group();
        Ok(PutUserInput {
            group: GroupId::new(group.host_relay_url(), group.local_id()),
            target_pubkey: root.target_pubkey().to_string(),
            // PRESENCE-PRESERVING: absent -> None, present-empty -> Some("").
            role: root.role().map(str::to_string),
            reason: root.reason().map(str::to_string),
        })
    }
}

// --- CreateInviteInput -------------------------------------------------------

impl ActionPayload for CreateInviteInput {
    const SCHEMA_ID: &'static str = "nmp.nip29.create_invite";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let host_relay_url = fbb.create_string(&self.group.host_relay_url);
        let local_id = fbb.create_string(&self.group.local_id);
        let group = invite_fb::GroupRef::create(
            &mut fbb,
            &invite_fb::GroupRefArgs {
                host_relay_url: Some(host_relay_url),
                local_id: Some(local_id),
            },
        );
        let code_offsets: Vec<_> = self.codes.iter().map(|c| fbb.create_string(c)).collect();
        let codes = fbb.create_vector(&code_offsets);
        let payload = invite_fb::CreateInvitePayload::create(
            &mut fbb,
            &invite_fb::CreateInvitePayloadArgs {
                schema_version: SCHEMA_VERSION,
                group: Some(group),
                codes: Some(codes),
            },
        );
        invite_fb::finish_create_invite_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !invite_fb::create_invite_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N29I file identifier"));
        }
        let root = invite_fb::root_as_create_invite_payload(bytes)
            .map_err(|e| malformed(format!("not a valid CreateInvitePayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let group = root.group();
        let codes = root
            .codes()
            .map(|v| v.iter().map(str::to_string).collect())
            .unwrap_or_default();
        Ok(CreateInviteInput {
            group: GroupId::new(group.host_relay_url(), group.local_id()),
            codes,
        })
    }
}
