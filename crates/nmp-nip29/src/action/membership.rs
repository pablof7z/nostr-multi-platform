//! User-self membership actions: `JoinRequest` (9021) and `LeaveRequest` (9022).
//!
//! Per `kinds.md` §2.2: both are signed by the prospective member / leaver, not
//! an admin. The relay reaction (auto-emit 39002, optionally consume invite
//! code) is server-side; the client just publishes the request.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{KIND_JOIN_REQUEST, KIND_LEAVE_REQUEST};

use super::publish_plan::PublishPlan;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JoinRequestInput {
    pub group: GroupId,
    #[serde(default)]
    pub invite_code: Option<String>,
    #[serde(default)]
    pub referrer_event_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Build the kind:9021 join-request `PublishPlan` from a typed input.
///
/// Single source of truth for the join-request tag layout: both
/// [`JoinRequestAction::start`] (validation) and [`join_request_command`]
/// (executor) consult it, so the wire shape can never drift between the two.
fn join_request_plan(action: &JoinRequestInput) -> PublishPlan {
    let mut tags = vec![vec!["h".into(), action.group.local_id.clone()]];
    if let Some(code) = &action.invite_code {
        tags.push(vec!["code".into(), code.clone()]);
    }
    if let Some(evt) = &action.referrer_event_id {
        tags.push(vec!["e".into(), evt.clone()]);
    }
    let content = action.reason.clone().unwrap_or_default();
    PublishPlan::pinned(&action.group, KIND_JOIN_REQUEST, content, tags)
}

/// Map a validated `nip29.join_request` action JSON to the [`ActorCommand`]
/// that publishes the kind:9021 join-request event, host-pinned to the
/// group's own relay. Re-decodes its own input — the executor never trusts an
/// upstream shape it did not verify.
pub fn join_request_command(action_json: &str) -> Result<ActorCommand, String> {
    let input: JoinRequestInput =
        serde_json::from_str(action_json).map_err(|e| e.to_string())?;
    join_request_plan(&input).into_actor_command()
}

pub struct JoinRequestAction;
impl ActionModule for JoinRequestAction {
    const NAMESPACE: &'static str = "nip29.join_request";
    type Action = JoinRequestInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        join_request_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for join request".into()))?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LeaveRequestInput {
    pub group: GroupId,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Build the kind:9022 leave-request `PublishPlan` from a typed input.
fn leave_request_plan(action: &LeaveRequestInput) -> PublishPlan {
    let tags = vec![vec!["h".into(), action.group.local_id.clone()]];
    PublishPlan::pinned(
        &action.group,
        KIND_LEAVE_REQUEST,
        action.reason.clone().unwrap_or_default(),
        tags,
    )
}

/// Map a validated `nip29.leave_request` action JSON to the [`ActorCommand`]
/// that publishes the kind:9022 leave-request event.
pub fn leave_request_command(action_json: &str) -> Result<ActorCommand, String> {
    let input: LeaveRequestInput =
        serde_json::from_str(action_json).map_err(|e| e.to_string())?;
    leave_request_plan(&input).into_actor_command()
}

pub struct LeaveRequestAction;
impl ActionModule for LeaveRequestAction {
    const NAMESPACE: &'static str = "nip29.leave_request";
    type Action = LeaveRequestInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        leave_request_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for leave request".into()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_request_command_emits_host_pinned_kind_9021() {
        let json = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"},"invite_code":"abc","referrer_event_id":"deadbeef","reason":"hi"}"#;
        match join_request_command(json).expect("well-formed join request") {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
                assert_eq!(event.kind, KIND_JOIN_REQUEST);
                assert!(event
                    .tags
                    .contains(&vec!["h".to_string(), "room".to_string()]));
                assert!(event
                    .tags
                    .contains(&vec!["code".to_string(), "abc".to_string()]));
                assert!(event
                    .tags
                    .contains(&vec!["e".to_string(), "deadbeef".to_string()]));
                assert_eq!(event.content, "hi");
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn leave_request_command_emits_host_pinned_kind_9022() {
        let json = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"}}"#;
        match leave_request_command(json).expect("well-formed leave request") {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
                assert_eq!(event.kind, KIND_LEAVE_REQUEST);
                assert_eq!(
                    event.tags,
                    vec![vec!["h".to_string(), "room".to_string()]]
                );
                assert_eq!(event.content, "");
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn join_request_command_rejects_malformed_body() {
        assert!(join_request_command(r#"{"reason":"no group"}"#).is_err());
    }
}
