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
        if action.group.host_relay_url.is_empty() {
            return Err(ActionRejection::Invalid(
                "join request needs a non-empty group.host_relay_url".into(),
            ));
        }
        if action.group.local_id.is_empty() {
            return Err(ActionRejection::Invalid(
                "join request needs a non-empty group.local_id".into(),
            ));
        }
        join_group_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for join request".into()))?;
        Ok(())
    }

    /// ADR-0027 — build the kind:9021 join-request publish plan and enqueue
    /// the host-pinned [`ActorCommand::PublishUnsignedEventToRelays`].
    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let cmd = join_group_plan(&action).into_actor_command()?;
        send(cmd);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::UnsignedEvent;

    fn input() -> JoinGroupInput {
        JoinGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            invite_code: None,
            reason: None,
        }
    }

    #[test]
    fn well_formed_input_yields_host_pinned_kind_9021_publish_command() {
        let body = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"}}"#;
        match join_group_command(body).expect("well-formed body parses") {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
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
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn invite_code_lands_as_code_tag() {
        let body = r#"{"group":{"host_relay_url":"wss://h","local_id":"r"},"invite_code":"secret-1"}"#;
        let cmd = join_group_command(body).expect("well-formed");
        let event: UnsignedEvent = match cmd {
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
        let body = r#"{"group":{"host_relay_url":"wss://h","local_id":"r"},"reason":"please let me in"}"#;
        let cmd = join_group_command(body).expect("well-formed");
        let event = match cmd {
            ActorCommand::PublishUnsignedEventToRelays { event, .. } => event,
            other => panic!("expected publish, got {other:?}"),
        };
        assert_eq!(event.content, "please let me in");
    }

    #[test]
    fn missing_host_relay_is_rejected_in_executor() {
        let body = r#"{"group":{"host_relay_url":"","local_id":"r"}}"#;
        // The executor builds a `PublishPlan::pinned` regardless and the
        // empty host gets through — the relay pin lane will reject downstream.
        // But the typed validator (below) rejects it first.
        let mut ctx = ActionContext { now_ms: 0 };
        let action: JoinGroupInput = serde_json::from_str(body).unwrap();
        assert!(matches!(
            JoinGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn missing_local_id_is_rejected_by_validator() {
        let mut ctx = ActionContext { now_ms: 0 };
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
        let mut ctx = ActionContext { now_ms: 0 };
        assert!(JoinGroupAction::start(&mut ctx, input()).is_ok());
    }

    #[test]
    fn malformed_json_is_rejected_by_executor() {
        assert!(join_group_command(r#"{"no":"group"}"#).is_err());
    }
}
