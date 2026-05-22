//! Group-join action: publish a kind:9021 (`join-request`) to a NIP-29 host
//! relay.
//!
//! Per `docs/design/nip29/kinds.md` §2.2:
//! - **Required tag:** `["h", group_id]`
//! - **Optional tag:** `["code", invite_code]` for preauthorized join
//! - **Content:** optional human-readable reason
//! - **Signer:** the prospective member (the active local identity)
//! - **Routing:** host relay (pin) — same Case-E lane as the user-content
//!   actions in `content.rs` / `composed.rs`.
//!
//! The relay's response is asynchronous: open + uncoded → it republishes
//! kind:39002 with the new member; closed + valid code → 39002 + code is
//! consumed; closed + no code → held for admin review. The UX layer reads
//! the resulting member set from
//! [`crate::projection::DiscoveredGroupsProjection`] (or a per-group
//! projection) — this action only emits the request.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::KIND_JOIN_REQUEST;

use super::publish_plan::PublishPlan;

/// Action input — the group to join, plus optional preauth code and reason.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JoinGroupInput {
    /// Target NIP-29 group identity (`{host_relay_url, local_id}`).
    pub group: GroupId,
    /// Optional invite code (a `["code", _]` tag on the request). Closed
    /// groups consume it on the first 9021 that uses it.
    #[serde(default)]
    pub invite_code: Option<String>,
    /// Optional human-readable reason. Empty / missing → no content.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Build the kind:9021 join-request `PublishPlan` from a typed input.
fn join_group_plan(action: &JoinGroupInput) -> PublishPlan {
    let mut tags = vec![vec!["h".into(), action.group.local_id.clone()]];
    if let Some(code) = &action.invite_code {
        tags.push(vec!["code".into(), code.clone()]);
    }
    let content = action.reason.clone().unwrap_or_default();
    PublishPlan::pinned(&action.group, KIND_JOIN_REQUEST, content, tags)
}

pub struct JoinGroupAction;
impl ActionModule for JoinGroupAction {
    const NAMESPACE: &'static str = "nmp.nip29.join";
    type Action = JoinGroupInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        // The host pin must be present and non-empty (a missing
        // `host_relay_url` would route the request through the NIP-65 outbox
        // — wrong relay, the join would never reach the host).
        action.group.require_routable().map_err(ActionRejection::Invalid)?;
        join_group_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for join request".into()))?;
        Ok(())
    }
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(join_group_plan(&action)
            .into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::UnsignedEvent;
    use std::cell::RefCell;

    fn input() -> JoinGroupInput {
        JoinGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            invite_code: None,
            reason: None,
        }
    }

    /// Run the typed executor and capture every `ActorCommand` it sends, in order.
    fn run_execute(input: JoinGroupInput) -> Result<Vec<ActorCommand>, String> {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        JoinGroupAction::execute(input, "test-cid", &|cmd| {
            captured.borrow_mut().push(cmd);
        })?;
        Ok(captured.into_inner())
    }

    #[test]
    fn well_formed_input_yields_host_pinned_kind_9021_publish_command() {
        let cmds = run_execute(input()).expect("well-formed input executes");
        assert_eq!(cmds.len(), 1, "join executor must send exactly one command, got {cmds:?}");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::PublishUnsignedEventToRelays { event, relays, correlation_id } => {
                // Pinned to EXACTLY the host relay — never the NIP-65 outbox.
                assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
                assert_eq!(event.kind, KIND_JOIN_REQUEST);
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
    fn invite_code_lands_as_code_tag() {
        let cmds = run_execute(JoinGroupInput {
            group: GroupId::new("wss://h", "r"),
            invite_code: Some("secret-1".to_string()),
            reason: None,
        })
        .expect("well-formed");
        let event: UnsignedEvent = match cmds.into_iter().next().expect("one command") {
            ActorCommand::PublishUnsignedEventToRelays { event, .. } => event,
            other => panic!("expected publish, got {other:?}"),
        };
        assert!(
            event.tags.iter().any(|t| t == &vec!["code".to_string(), "secret-1".to_string()]),
            "must carry the ['code', invite_code] tag, got {:?}",
            event.tags
        );
    }

    #[test]
    fn reason_lands_in_content() {
        let cmds = run_execute(JoinGroupInput {
            group: GroupId::new("wss://h", "r"),
            invite_code: None,
            reason: Some("please let me in".to_string()),
        })
        .expect("well-formed");
        let event = match cmds.into_iter().next().expect("one command") {
            ActorCommand::PublishUnsignedEventToRelays { event, .. } => event,
            other => panic!("expected publish, got {other:?}"),
        };
        assert_eq!(event.content, "please let me in");
    }

    #[test]
    fn missing_host_relay_is_rejected_by_validator() {
        let mut ctx = ActionContext::default();
        let action = JoinGroupInput {
            group: GroupId::new("", "r"),
            invite_code: None,
            reason: None,
        };
        assert!(matches!(
            JoinGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn missing_local_id_is_rejected_by_validator() {
        let mut ctx = ActionContext::default();
        let action = JoinGroupInput {
            group: GroupId::new("wss://h", ""),
            invite_code: None,
            reason: None,
        };
        assert!(matches!(
            JoinGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn well_formed_passes_validator() {
        let mut ctx = ActionContext::default();
        assert!(JoinGroupAction::start(&mut ctx, input()).is_ok());
    }
}
