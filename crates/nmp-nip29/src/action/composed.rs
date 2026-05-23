//! `ReactInGroup` (kind:7+h), `CommentInGroup` (kind:1111+h).
//!
//! These are the "host-pinned variant of an otherwise cross-protocol action"
//! per `kinds.md` §4. They live here because the routing concern (the `h`
//! tag) is the discriminator.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{KIND_COMMENT, KIND_REACTION};

use super::publish_plan::PublishPlan;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReactInGroupInput {
    pub group: GroupId,
    pub target_event_id: String,
    pub target_author_pubkey: Option<String>,
    pub content: String,
}

/// Build the kind:7 in-group reaction `PublishPlan` from a typed input.
fn react_in_group_plan(action: &ReactInGroupInput) -> PublishPlan {
    let mut tags = vec![
        vec!["h".into(), action.group.local_id.clone()],
        vec!["e".into(), action.target_event_id.clone()],
    ];
    if let Some(p) = &action.target_author_pubkey {
        tags.push(vec!["p".into(), p.clone()]);
    }
    PublishPlan::pinned(&action.group, KIND_REACTION, action.content.clone(), tags)
}

pub struct ReactInGroupAction;
impl ActionModule for ReactInGroupAction {
    const NAMESPACE: &'static str = "nmp.nip29.react_in_group";
    type Action = ReactInGroupInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        action.group.require_routable().map_err(ActionRejection::Invalid)?;
        if action.target_event_id.is_empty() {
            return Err(ActionRejection::Invalid("target_event_id is empty".into()));
        }
        if action.content.is_empty() {
            return Err(ActionRejection::Invalid("reaction content is empty".into()));
        }
        react_in_group_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for in-group reaction".into()))?;
        Ok(())
    }
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(react_in_group_plan(&action)
            .into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CommentInGroupInput {
    pub group: GroupId,
    pub root_event_id: Option<String>,
    pub parent_event_id: Option<String>,
    pub content: String,
}

/// Build the kind:1111 in-group comment `PublishPlan` from a typed input.
fn comment_in_group_plan(action: &CommentInGroupInput) -> PublishPlan {
    let mut tags = vec![vec!["h".into(), action.group.local_id.clone()]];
    if let Some(root) = &action.root_event_id {
        tags.push(vec!["E".into(), root.clone()]);
    }
    if let Some(parent) = &action.parent_event_id {
        tags.push(vec!["e".into(), parent.clone()]);
    }
    PublishPlan::pinned(&action.group, KIND_COMMENT, action.content.clone(), tags)
}

pub struct CommentInGroupAction;
impl ActionModule for CommentInGroupAction {
    const NAMESPACE: &'static str = "nmp.nip29.comment_in_group";
    type Action = CommentInGroupInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        action.group.require_routable().map_err(ActionRejection::Invalid)?;
        comment_in_group_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for in-group comment".into()))?;
        Ok(())
    }
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(comment_in_group_plan(&action)
            .into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds::{KIND_COMMENT, KIND_REACTION};
    use std::cell::RefCell;

    fn react_input() -> ReactInGroupInput {
        ReactInGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            target_event_id: "deadbeef".to_string(),
            target_author_pubkey: None,
            content: "+".to_string(),
        }
    }

    fn comment_input() -> CommentInGroupInput {
        CommentInGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            root_event_id: None,
            parent_event_id: None,
            content: "nice".to_string(),
        }
    }

    #[test]
    fn react_well_formed_passes_validator() {
        let mut ctx = ActionContext::default();
        assert!(ReactInGroupAction::start(&mut ctx, react_input()).is_ok());
    }

    #[test]
    fn react_empty_host_relay_url_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let action = ReactInGroupInput {
            group: GroupId::new("", "room"),
            ..react_input()
        };
        assert!(matches!(
            ReactInGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_empty_local_id_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let action = ReactInGroupInput {
            group: GroupId::new("wss://h", ""),
            ..react_input()
        };
        assert!(matches!(
            ReactInGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_empty_target_event_id_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let action = ReactInGroupInput {
            target_event_id: String::new(),
            ..react_input()
        };
        assert!(matches!(
            ReactInGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_empty_content_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let action = ReactInGroupInput {
            content: String::new(),
            ..react_input()
        };
        assert!(matches!(
            ReactInGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn comment_well_formed_passes_validator() {
        let mut ctx = ActionContext::default();
        assert!(CommentInGroupAction::start(&mut ctx, comment_input()).is_ok());
    }

    #[test]
    fn comment_empty_host_relay_url_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let action = CommentInGroupInput {
            group: GroupId::new("", "room"),
            ..comment_input()
        };
        assert!(matches!(
            CommentInGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn comment_empty_local_id_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let action = CommentInGroupInput {
            group: GroupId::new("wss://h", ""),
            ..comment_input()
        };
        assert!(matches!(
            CommentInGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_execute_emits_host_pinned_kind7_publish_command() {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        ReactInGroupAction::execute(react_input(), "react-cid", &|cmd| {
            captured.borrow_mut().push(cmd);
        })
        .expect("well-formed input executes");
        let cmds = captured.into_inner();
        assert_eq!(cmds.len(), 1, "react executor must send exactly one command, got {cmds:?}");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::PublishUnsignedEventToRelays { event, relays, correlation_id } => {
                assert_eq!(event.kind, KIND_REACTION, "react must emit kind:7");
                assert_eq!(
                    relays,
                    vec!["wss://groups.example.com".to_string()],
                    "react must be pinned to the group's host relay"
                );
                assert!(
                    event.tags.iter().any(|t| t == &["h".to_string(), "room".to_string()]),
                    "must carry the ['h', local_id] group tag, got {:?}",
                    event.tags
                );
                assert_eq!(event.content, "+");
                assert_eq!(correlation_id.as_deref(), Some("react-cid"));
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn comment_execute_emits_host_pinned_kind1111_publish_command() {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        CommentInGroupAction::execute(comment_input(), "comment-cid", &|cmd| {
            captured.borrow_mut().push(cmd);
        })
        .expect("well-formed input executes");
        let cmds = captured.into_inner();
        assert_eq!(cmds.len(), 1, "comment executor must send exactly one command, got {cmds:?}");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::PublishUnsignedEventToRelays { event, relays, correlation_id } => {
                assert_eq!(event.kind, KIND_COMMENT, "comment must emit kind:1111");
                assert_eq!(
                    relays,
                    vec!["wss://groups.example.com".to_string()],
                    "comment must be pinned to the group's host relay"
                );
                assert!(
                    event.tags.iter().any(|t| t == &["h".to_string(), "room".to_string()]),
                    "must carry the ['h', local_id] group tag, got {:?}",
                    event.tags
                );
                assert_eq!(event.content, "nice");
                assert_eq!(correlation_id.as_deref(), Some("comment-cid"));
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }
}
