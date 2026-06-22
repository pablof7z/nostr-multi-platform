//! Neutral host-pinned group-event producers.
//!
//! These actions own only the NIP-29 `h`-tag routing concern. Kind 11 and kind
//! 16 semantics stay protocol-neutral; downstream crates decide whether a row is
//! an article share, media share, repost, or something else.

use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::KIND_DISCUSSION_OR_ARTIFACT;

use super::publish_plan::PublishPlan;

const REPOST_KIND: u32 = 16;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GroupEventTarget {
    pub event_id: String,
    #[serde(default)]
    pub author_pubkey: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ShareEventInGroupInput {
    pub group: GroupId,
    pub target: GroupEventTarget,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub additional_tags: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RepostInGroupInput {
    pub group: GroupId,
    pub target: GroupEventTarget,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub additional_tags: Vec<Vec<String>>,
}

fn group_event_tags(
    group: &GroupId,
    target: &GroupEventTarget,
    additional_tags: &[Vec<String>],
) -> Vec<Vec<String>> {
    let mut tags = vec![
        vec!["h".into(), group.local_id.clone()],
        vec!["e".into(), target.event_id.clone()],
    ];
    if let Some(author) = &target.author_pubkey {
        tags.push(vec!["p".into(), author.clone()]);
    }
    tags.extend(additional_tags.iter().cloned());
    tags
}

fn validate_group_event_input(
    group: &GroupId,
    target: &GroupEventTarget,
    additional_tags: &[Vec<String>],
) -> Result<(), ActionRejection> {
    group.require_routable().map_err(ActionRejection::Invalid)?;
    if target.event_id.is_empty() {
        return Err(ActionRejection::Invalid("target event_id is empty".into()));
    }
    if additional_tags
        .iter()
        .any(|tag| tag.first().is_some_and(|key| key == "h"))
    {
        return Err(ActionRejection::Invalid(
            "additional_tags must not override the group h tag".into(),
        ));
    }
    Ok(())
}

fn share_event_plan(action: &ShareEventInGroupInput) -> PublishPlan {
    PublishPlan::pinned(
        &action.group,
        KIND_DISCUSSION_OR_ARTIFACT,
        action.content.clone(),
        group_event_tags(&action.group, &action.target, &action.additional_tags),
    )
}

fn repost_plan(action: &RepostInGroupInput) -> PublishPlan {
    PublishPlan::pinned(
        &action.group,
        REPOST_KIND,
        action.content.clone(),
        group_event_tags(&action.group, &action.target, &action.additional_tags),
    )
}

pub struct ShareEventInGroupAction;
impl ActionModule for ShareEventInGroupAction {
    const NAMESPACE: &'static str = "nmp.nip29.share_event_in_group";
    type Action = ShareEventInGroupInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<ShareEventInGroupInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_group_event_input(&action.group, &action.target, &action.additional_tags)?;
        share_event_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for in-group share".into()))
    }

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(share_event_plan(&action).into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

pub struct RepostInGroupAction;
impl ActionModule for RepostInGroupAction {
    const NAMESPACE: &'static str = "nmp.nip29.repost_in_group";
    type Action = RepostInGroupInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<RepostInGroupInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_group_event_input(&action.group, &action.target, &action.additional_tags)?;
        repost_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for in-group repost".into()))
    }

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(repost_plan(&action).into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn target() -> GroupEventTarget {
        GroupEventTarget {
            event_id: "target-id".into(),
            author_pubkey: Some("target-author".into()),
        }
    }

    fn share_input() -> ShareEventInGroupInput {
        ShareEventInGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            target: target(),
            content: "shared".into(),
            additional_tags: vec![vec!["t".into(), "nostr".into()]],
        }
    }

    #[test]
    fn share_rejects_empty_target() {
        let mut ctx = ActionContext::default();
        let action = ShareEventInGroupInput {
            target: GroupEventTarget::default(),
            ..share_input()
        };
        assert!(matches!(
            ShareEventInGroupAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn share_rejects_duplicate_h_in_additional_tags() {
        let mut ctx = ActionContext::default();
        let action = ShareEventInGroupInput {
            additional_tags: vec![vec!["h".into(), "other".into()]],
            ..share_input()
        };
        assert!(matches!(
            ShareEventInGroupAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn share_executes_host_pinned_kind11() {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        ShareEventInGroupAction
            .execute(share_input(), "share-cid", &|cmd| {
                captured.borrow_mut().push(cmd)
            })
            .expect("share executes");

        match captured.into_inner().pop().expect("command emitted") {
            ActorCommand::PublishUnsignedEventToRelays {
                event,
                relays,
                correlation_id,
                ..
            } => {
                assert_eq!(event.kind, KIND_DISCUSSION_OR_ARTIFACT);
                assert_eq!(relays, vec!["wss://groups.example.com"]);
                assert!(event.tags.iter().any(|t| t == &["h", "room"]));
                assert!(event.tags.iter().any(|t| t == &["e", "target-id"]));
                assert!(event.tags.iter().any(|t| t == &["p", "target-author"]));
                assert!(event.tags.iter().any(|t| t == &["t", "nostr"]));
                assert_eq!(correlation_id.as_deref(), Some("share-cid"));
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn repost_executes_host_pinned_kind16() {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        let action = RepostInGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            target: target(),
            content: String::new(),
            additional_tags: Vec::new(),
        };
        RepostInGroupAction
            .execute(action, "repost-cid", &|cmd| captured.borrow_mut().push(cmd))
            .expect("repost executes");

        match captured.into_inner().pop().expect("command emitted") {
            ActorCommand::PublishUnsignedEventToRelays {
                event,
                relays,
                correlation_id,
                ..
            } => {
                assert_eq!(event.kind, REPOST_KIND);
                assert_eq!(relays, vec!["wss://groups.example.com"]);
                assert!(event.tags.iter().any(|t| t == &["h", "room"]));
                assert!(event.tags.iter().any(|t| t == &["e", "target-id"]));
                assert_eq!(correlation_id.as_deref(), Some("repost-cid"));
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }
}
