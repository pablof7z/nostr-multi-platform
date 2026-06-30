//! `ReactInGroup` (kind:7+h) and `UnreactInGroup` (kind:5+h).
//!
//! These are the "host-pinned variant of an otherwise cross-protocol action"
//! per `kinds.md` §4. They live here because the routing concern (the `h`
//! tag) is the discriminator. Each is a thin convenience over the generic
//! group-publish route: the envelope (`h` / `previous` / pin) is composed by
//! [`super::publish::group_publish_plan`]; the action only shapes its
//! kind-specific caller tags.
//!
//! `ReactInGroup` adds a kind:7 reaction (`e` / `p` caller tags). `UnreactInGroup`
//! is the toggle-off: a NIP-09 kind:5 deletion of the viewer's own kind:7
//! (`e` target + `k:7` caller tags). NIP-29 owns only the `h`-tag routing + host
//! pin — emitting the delete through `group_publish_plan` is what makes the
//! group relay accept the retraction (a bare, un-pinned, un-`h`-tagged kind:5 is
//! rejected). The reaction-delete semantics stay NIP-25/NIP-09.

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;

use super::publish::group_publish_plan;
use super::publish_plan::PublishPlan;

/// NIP-25 reaction kind. Kept file-private to `composed.rs` because NIP-29
/// does not own kind:7 — it only adds the `h`-tag routing concern. The
/// producer for the `h`-tagged variant lives here per `kinds.md` §4; the
/// kind constant itself stays inlined to avoid asserting NIP-29 ownership
/// over a foreign-NIP kind.
const REACTION_KIND: u32 = 7;

/// NIP-09 deletion (kind:5). Kept file-private for the same reason as
/// [`REACTION_KIND`]: NIP-29 owns only the `h`-tag routing, not the deletion
/// semantics. The `unreact_in_group` toggle-off emits a kind:5 deleting the
/// viewer's own kind:7.
const DELETE_KIND: u32 = 5;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReactInGroupInput {
    pub group: GroupId,
    pub target_event_id: String,
    pub target_author_pubkey: Option<String>,
    pub content: String,
}

/// The kind:7 reaction's caller tags (`e` target, optional `p` author). The
/// NIP-29 envelope tags are injected by [`group_publish_plan`].
fn react_caller_tags(action: &ReactInGroupInput) -> Vec<Vec<String>> {
    let mut tags = vec![vec!["e".to_string(), action.target_event_id.clone()]];
    if let Some(p) = &action.target_author_pubkey {
        tags.push(vec!["p".to_string(), p.clone()]);
    }
    tags
}

/// Build the kind:7 in-group reaction `PublishPlan`, composing the NIP-29
/// envelope (`h` / `previous` / pin) from the store cache.
fn react_in_group_plan(ctx: &ActionContext, action: &ReactInGroupInput) -> PublishPlan {
    group_publish_plan(
        ctx,
        &action.group,
        REACTION_KIND,
        action.content.clone(),
        react_caller_tags(action),
    )
}

pub struct ReactInGroupAction;

impl ActionModule for ReactInGroupAction {
    const NAMESPACE: &'static str = "nmp.nip29.react_in_group";
    type Action = ReactInGroupInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<ReactInGroupInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        action
            .group
            .require_routable()
            .map_err(ActionRejection::Invalid)?;
        if action.target_event_id.is_empty() {
            return Err(ActionRejection::Invalid("target_event_id is empty".into()));
        }
        if action.content.is_empty() {
            return Err(ActionRejection::Invalid("reaction content is empty".into()));
        }
        Ok(())
    }
    fn execute(
        &self,
        ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(
            react_in_group_plan(ctx, &action)
                .into_actor_command(Some(correlation_id.to_string()))?,
        );
        Ok(())
    }
}

/// Typed input for [`UnreactInGroupAction`] — the reaction toggle-off.
///
/// `reaction_event_id` is the viewer's OWN kind:7 reaction event id to delete
/// (the app reads it from the reaction aggregate's per-target `mine` handle).
/// The deletion is published as a host-pinned, `h`-tagged kind:5.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct UnreactInGroupInput {
    pub group: GroupId,
    pub reaction_event_id: String,
}

/// The NIP-09 deletion's caller tags: `["e", reaction_event_id]` naming the
/// kind:7 to delete, plus the recommended `["k", "7"]` deleted-kind hint. The
/// NIP-29 envelope tags (`h` / `previous`) are injected by [`group_publish_plan`].
fn unreact_caller_tags(action: &UnreactInGroupInput) -> Vec<Vec<String>> {
    vec![
        vec!["e".to_string(), action.reaction_event_id.clone()],
        vec!["k".to_string(), REACTION_KIND.to_string()],
    ]
}

/// Build the kind:5 in-group retraction `PublishPlan`, composing the NIP-29
/// envelope (`h` / `previous` / pin) from the store cache. The kind:5 carries no
/// content (a NIP-09 deletion reason is not modelled on this toggle-off path).
fn unreact_in_group_plan(ctx: &ActionContext, action: &UnreactInGroupInput) -> PublishPlan {
    group_publish_plan(
        ctx,
        &action.group,
        DELETE_KIND,
        String::new(),
        unreact_caller_tags(action),
    )
}

pub struct UnreactInGroupAction;

impl ActionModule for UnreactInGroupAction {
    const NAMESPACE: &'static str = "nmp.nip29.unreact_in_group";
    type Action = UnreactInGroupInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<UnreactInGroupInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        action
            .group
            .require_routable()
            .map_err(ActionRejection::Invalid)?;
        if action.reaction_event_id.is_empty() {
            return Err(ActionRejection::Invalid("reaction_event_id is empty".into()));
        }
        Ok(())
    }

    fn execute(
        &self,
        ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(
            unreact_in_group_plan(ctx, &action)
                .into_actor_command(Some(correlation_id.to_string()))?,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::actor::PublishCommand;
    use std::cell::RefCell;

    fn action() -> ReactInGroupAction {
        ReactInGroupAction
    }

    fn react_input() -> ReactInGroupInput {
        ReactInGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            target_event_id: "deadbeef".to_string(),
            target_author_pubkey: None,
            content: "+".to_string(),
        }
    }

    #[test]
    fn react_well_formed_passes_validator() {
        let mut ctx = ActionContext::default();
        assert!(action().start(&mut ctx, react_input()).is_ok());
    }

    #[test]
    fn react_empty_host_relay_url_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let input = ReactInGroupInput {
            group: GroupId::new("", "room"),
            ..react_input()
        };
        assert!(matches!(
            action().start(&mut ctx, input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_empty_local_id_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let input = ReactInGroupInput {
            group: GroupId::new("wss://h", ""),
            ..react_input()
        };
        assert!(matches!(
            action().start(&mut ctx, input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_empty_target_event_id_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let input = ReactInGroupInput {
            target_event_id: String::new(),
            ..react_input()
        };
        assert!(matches!(
            action().start(&mut ctx, input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_empty_content_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let input = ReactInGroupInput {
            content: String::new(),
            ..react_input()
        };
        assert!(matches!(
            action().start(&mut ctx, input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_execute_emits_host_pinned_kind7_publish_command() {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        let ctx = ActionContext::default();
        action()
            .execute(&ctx, react_input(), "react-cid", &|cmd| {
                captured.borrow_mut().push(cmd);
            })
            .expect("well-formed input executes");
        let cmds = captured.into_inner();
        assert_eq!(
            cmds.len(),
            1,
            "react executor must send exactly one command, got {cmds:?}"
        );
        match cmds.into_iter().next().unwrap() {
            ActorCommand::Publish(PublishCommand::UnsignedEventToRelays {
                event,
                relays,
                correlation_id,
                ..
            }) => {
                assert_eq!(event.kind, REACTION_KIND, "react must emit kind:7");
                assert_eq!(
                    relays,
                    vec!["wss://groups.example.com".to_string()],
                    "react must be pinned to the group's host relay"
                );
                assert!(
                    event
                        .tags
                        .iter()
                        .any(|t| t == &["h".to_string(), "room".to_string()]),
                    "must carry the ['h', local_id] group tag, got {:?}",
                    event.tags
                );
                assert_eq!(event.content, "+");
                assert_eq!(correlation_id.as_deref(), Some("react-cid"));
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    fn unreact_input() -> UnreactInGroupInput {
        UnreactInGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            reaction_event_id: "ab".repeat(32),
        }
    }

    #[test]
    fn unreact_well_formed_passes_validator() {
        let mut ctx = ActionContext::default();
        assert!(UnreactInGroupAction
            .start(&mut ctx, unreact_input())
            .is_ok());
    }

    #[test]
    fn unreact_empty_host_relay_url_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let input = UnreactInGroupInput {
            group: GroupId::new("", "room"),
            ..unreact_input()
        };
        assert!(matches!(
            UnreactInGroupAction.start(&mut ctx, input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn unreact_empty_reaction_event_id_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let input = UnreactInGroupInput {
            reaction_event_id: String::new(),
            ..unreact_input()
        };
        assert!(matches!(
            UnreactInGroupAction.start(&mut ctx, input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn unreact_execute_emits_host_pinned_h_tagged_kind5_delete() {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        let ctx = ActionContext::default();
        UnreactInGroupAction
            .execute(&ctx, unreact_input(), "unreact-cid", &|cmd| {
                captured.borrow_mut().push(cmd);
            })
            .expect("well-formed input executes");
        let cmds = captured.into_inner();
        assert_eq!(cmds.len(), 1, "exactly one publish, got {cmds:?}");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::Publish(PublishCommand::UnsignedEventToRelays {
                event,
                relays,
                correlation_id,
                ..
            }) => {
                assert_eq!(event.kind, DELETE_KIND, "must emit kind:5 (NIP-09 delete)");
                assert_eq!(
                    relays,
                    vec!["wss://groups.example.com".to_string()],
                    "retraction must be pinned to the group's host relay"
                );
                assert!(
                    event
                        .tags
                        .iter()
                        .any(|t| t == &["h".to_string(), "room".to_string()]),
                    "must carry the ['h', local_id] group tag, got {:?}",
                    event.tags
                );
                assert!(
                    event
                        .tags
                        .iter()
                        .any(|t| t.first().map(String::as_str) == Some("e")
                            && t.get(1).map(String::as_str) == Some(&"ab".repeat(32))),
                    "must delete the viewer's own kind:7 by id, got {:?}",
                    event.tags
                );
                assert!(
                    event
                        .tags
                        .iter()
                        .any(|t| t == &["k".to_string(), "7".to_string()]),
                    "must carry the ['k','7'] deleted-kind hint, got {:?}",
                    event.tags
                );
                assert_eq!(correlation_id.as_deref(), Some("unreact-cid"));
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }
}
