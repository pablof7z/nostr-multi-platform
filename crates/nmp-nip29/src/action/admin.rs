//! Admin actions for the ADR-0060 NIP-29 increment.
//!
//! This module intentionally implements only the two admin actions accepted by
//! ADR-0060: kind:9000 (`PutUser`) and kind:9009 (`CreateInvite`). Both are
//! structurally validated and host-pinned here. Relay-enforced admin authority
//! remains reflected through relay-signed 39001/39002 snapshots.

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{KIND_CREATE_INVITE, KIND_PUT_USER};

use super::publish_plan::PublishPlan;

/// Highlighter/relay29-compatible cap for one kind:9009 event.
pub const MAX_CODES_PER_INVITE_EVENT: usize = 10;

/// Add a user to a group, optionally granting a role on the same `p` tag.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PutUserInput {
    pub group: GroupId,
    pub target_pubkey: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Mint one or more invite codes. More than ten codes fan out across multiple
/// kind:9009 events because relay29 caps `code` tags per event.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct CreateInviteInput {
    pub group: GroupId,
    pub codes: Vec<String>,
}

fn put_user_plan(action: &PutUserInput) -> PublishPlan {
    let mut p_tag = vec!["p".to_string(), action.target_pubkey.clone()];
    if let Some(role) = action
        .role
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        p_tag.push(role.to_string());
    }

    let mut tags = vec![vec!["h".to_string(), action.group.local_id.clone()], p_tag];
    if let Some(reason) = action
        .reason
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        tags.push(vec!["reason".to_string(), reason.to_string()]);
    }

    PublishPlan::pinned(&action.group, KIND_PUT_USER, "", tags)
}

fn create_invite_plans(action: &CreateInviteInput) -> Vec<PublishPlan> {
    action
        .codes
        .chunks(MAX_CODES_PER_INVITE_EVENT)
        .map(|chunk| {
            let mut tags = vec![vec!["h".to_string(), action.group.local_id.clone()]];
            tags.extend(
                chunk
                    .iter()
                    .map(|code| vec!["code".to_string(), code.clone()]),
            );
            PublishPlan::pinned(&action.group, KIND_CREATE_INVITE, "", tags)
        })
        .collect()
}

fn validate_group(group: &GroupId) -> Result<(), ActionRejection> {
    group.require_routable().map_err(ActionRejection::Invalid)?;
    if !(group.host_relay_url.starts_with("wss://") || group.host_relay_url.starts_with("ws://")) {
        return Err(ActionRejection::Invalid(
            "group.host_relay_url must start with wss:// or ws://".into(),
        ));
    }
    Ok(())
}

fn validate_put_user(action: &PutUserInput) -> Result<(), ActionRejection> {
    validate_group(&action.group)?;
    if !is_hex64(&action.target_pubkey) {
        return Err(ActionRejection::Invalid(
            "target_pubkey must be 64 lowercase hex characters".into(),
        ));
    }
    if action.role.as_ref().is_some_and(|r| r.trim().is_empty()) {
        return Err(ActionRejection::Invalid("role must not be empty".into()));
    }
    put_user_plan(action)
        .validate_no_unpinned_h()
        .map_err(|_| ActionRejection::Invalid("missing host pin for put-user".into()))
}

fn validate_create_invite(action: &CreateInviteInput) -> Result<(), ActionRejection> {
    validate_group(&action.group)?;
    if action.codes.is_empty() {
        return Err(ActionRejection::Invalid(
            "at least one invite code is required".into(),
        ));
    }
    if action.codes.iter().any(|code| !is_valid_invite_code(code)) {
        return Err(ActionRejection::Invalid(
            "invite codes must be non-empty printable ASCII strings without whitespace".into(),
        ));
    }
    for plan in create_invite_plans(action) {
        plan.validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for create-invite".into()))?;
    }
    Ok(())
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_valid_invite_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 128
        && code == code.trim()
        && code
            .bytes()
            .all(|b| b.is_ascii_graphic() && !b.is_ascii_whitespace())
}

#[derive(Default)]
pub struct PutUserAction;

impl ActionModule for PutUserAction {
    const NAMESPACE: &'static str = "nmp.nip29.put_user";
    type Action = PutUserInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<PutUserInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_put_user(&action)
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(put_user_plan(&action).into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

#[derive(Default)]
pub struct CreateInviteAction;

impl ActionModule for CreateInviteAction {
    const NAMESPACE: &'static str = "nmp.nip29.create_invite";
    type Action = CreateInviteInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<CreateInviteInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_create_invite(&action)
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let cid = Some(correlation_id.to_string());
        for plan in create_invite_plans(&action) {
            send(plan.into_actor_command(cid.clone())?);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "admin/tests.rs"]
mod tests;
