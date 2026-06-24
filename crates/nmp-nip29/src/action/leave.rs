//! Group-leave action: publish a kind:9022 (`leave-request`) to a NIP-29 host
//! relay.
//!
//! Per `docs/design/nip29/kinds.md` §2.2:
//! - **Required tag:** `["h", group_id]`
//! - **Content:** optional human-readable reason
//! - **Signer:** the departing member (the active local identity)
//! - **Routing:** host relay (pin) — same Case-E lane as the user-content
//!   actions in `content.rs` / `composed.rs` and the symmetric `join.rs`.
//!
//! The relay's response is asynchronous: it republishes kind:39002 without the
//! departed member. The UX layer reads the resulting member set from
//! [`crate::projection::DiscoveredGroupsProjection`] (or a per-group
//! projection) — this action only emits the request.

use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};
use nmp_core::actor::{ActorCommand, PublishCommand};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::KIND_LEAVE_REQUEST;

use super::publish_plan::PublishPlan;

/// Action input — the group to leave, plus an optional human-readable reason.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LeaveGroupInput {
    /// Target NIP-29 group identity (`{host_relay_url, local_id}`).
    pub group: GroupId,
    /// Optional human-readable reason. Empty / missing → no content.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Build the kind:9022 leave-request `PublishPlan` from a typed input.
fn leave_group_plan(action: &LeaveGroupInput) -> PublishPlan {
    let tags = vec![vec!["h".into(), action.group.local_id.clone()]];
    let content = action.reason.clone().unwrap_or_default();
    PublishPlan::pinned(&action.group, KIND_LEAVE_REQUEST, content, tags)
}

#[derive(Default)]
pub struct LeaveGroupAction;
impl ActionModule for LeaveGroupAction {
    const NAMESPACE: &'static str = "nmp.nip29.leave";
    type Action = LeaveGroupInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<LeaveGroupInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        // The host pin must be present and non-empty (a missing
        // `host_relay_url` would route the request through the NIP-65 outbox
        // — wrong relay, the leave would never reach the host).
        action
            .group
            .require_routable()
            .map_err(ActionRejection::Invalid)?;
        leave_group_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for leave request".into()))?;
        Ok(())
    }
    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(leave_group_plan(&action).into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn input() -> LeaveGroupInput {
        LeaveGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            reason: None,
        }
    }

    /// Run the typed executor and capture every `ActorCommand` it sends, in order.
    fn run_execute(input: LeaveGroupInput) -> Result<Vec<ActorCommand>, String> {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        LeaveGroupAction.execute(input, "test-cid", &|cmd| {
            captured.borrow_mut().push(cmd);
        })?;
        Ok(captured.into_inner())
    }

    #[test]
    fn well_formed_input_yields_host_pinned_kind_9022_publish_command() {
        let cmds = run_execute(input()).expect("well-formed input executes");
        assert_eq!(
            cmds.len(),
            1,
            "leave executor must send exactly one command, got {cmds:?}"
        );
        match cmds.into_iter().next().unwrap() {
            ActorCommand::Publish(PublishCommand::UnsignedEventToRelays {
                event,
                relays,
                correlation_id,
                ..
            }) => {
                // Pinned to EXACTLY the host relay — never the NIP-65 outbox.
                assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
                assert_eq!(event.kind, KIND_LEAVE_REQUEST);
                assert!(
                    event
                        .tags
                        .iter()
                        .any(|t| t == &vec!["h".to_string(), "room".to_string()]),
                    "must carry the ['h', local_id] group tag, got {:?}",
                    event.tags
                );
                // No reason → empty content.
                assert_eq!(event.content, "");
                // Actor fills the pubkey at sign time.
                assert!(event.pubkey.is_empty());
                // correlation_id threads through from the executor.
                assert_eq!(correlation_id.as_deref(), Some("test-cid"));
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn reason_lands_in_content() {
        let cmds = run_execute(LeaveGroupInput {
            group: GroupId::new("wss://h", "r"),
            reason: Some("moving on".to_string()),
        })
        .expect("well-formed");
        let event = match cmds.into_iter().next().expect("one command") {
            ActorCommand::Publish(PublishCommand::UnsignedEventToRelays { event, .. }) => event,
            other => panic!("expected publish, got {other:?}"),
        };
        assert_eq!(event.content, "moving on");
    }

    #[test]
    fn missing_host_relay_is_rejected_by_validator() {
        let mut ctx = ActionContext::default();
        let action = LeaveGroupInput {
            group: GroupId::new("", "r"),
            reason: None,
        };
        assert!(matches!(
            LeaveGroupAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn missing_local_id_is_rejected_by_validator() {
        let mut ctx = ActionContext::default();
        let action = LeaveGroupInput {
            group: GroupId::new("wss://h", ""),
            reason: None,
        };
        assert!(matches!(
            LeaveGroupAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn well_formed_passes_validator() {
        let mut ctx = ActionContext::default();
        assert!(LeaveGroupAction.start(&mut ctx, input()).is_ok());
    }
}
