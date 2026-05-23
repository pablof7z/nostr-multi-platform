//! User-sent content action: chat (kind:9).

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::cache::previous_tag_prefix;
use crate::group_id::GroupId;
use crate::kinds::KIND_CHAT_MESSAGE;

use super::publish_plan::PublishPlan;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PostChatMessageInput {
    pub group: GroupId,
    pub content: String,
    /// Up to N event-id prefixes from the per-group `RecentGroupEvents`
    /// cache, per `moderation.md` §2.1. The caller (Session/SafeHighlighterCore
    /// equivalent) pulls these from `cache::RecentGroupEvents::previous_tags_for`
    /// before invoking the action; we accept them as input so the action stays
    /// pure (no cache reads inside `start`).
    #[serde(default)]
    pub previous_event_id_prefixes: Vec<String>,
    #[serde(default)]
    pub reply_to_event_id: Option<String>,
}

/// Build the kind:9 chat-message `PublishPlan` from a typed input.
fn post_chat_message_plan(action: &PostChatMessageInput) -> PublishPlan {
    let mut tags = vec![vec!["h".into(), action.group.local_id.clone()]];
    for prefix in &action.previous_event_id_prefixes {
        tags.push(vec!["previous".into(), previous_tag_prefix(prefix)]);
    }
    if let Some(reply) = &action.reply_to_event_id {
        tags.push(vec!["e".into(), reply.clone(), String::new(), "reply".into()]);
    }
    PublishPlan::pinned(&action.group, KIND_CHAT_MESSAGE, action.content.clone(), tags)
}

pub struct PostChatMessageAction;
impl ActionModule for PostChatMessageAction {
    const NAMESPACE: &'static str = "nmp.nip29.post_chat_message";
    type Action = PostChatMessageInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        action.group.require_routable().map_err(ActionRejection::Invalid)?;
        if action.content.is_empty() {
            return Err(ActionRejection::Invalid("empty chat message".into()));
        }
        post_chat_message_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for chat message".into()))?;
        Ok(())
    }
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(post_chat_message_plan(&action)
            .into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> PostChatMessageInput {
        PostChatMessageInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            content: "hello".to_string(),
            previous_event_id_prefixes: Vec::new(),
            reply_to_event_id: None,
        }
    }

    #[test]
    fn well_formed_passes_validator() {
        let mut ctx = ActionContext::default();
        assert!(PostChatMessageAction::start(&mut ctx, input()).is_ok());
    }

    #[test]
    fn empty_host_relay_url_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let action = PostChatMessageInput {
            group: GroupId::new("", "room"),
            ..input()
        };
        assert!(matches!(
            PostChatMessageAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn empty_local_id_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let action = PostChatMessageInput {
            group: GroupId::new("wss://h", ""),
            ..input()
        };
        assert!(matches!(
            PostChatMessageAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn empty_content_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let action = PostChatMessageInput {
            content: String::new(),
            ..input()
        };
        assert!(matches!(
            PostChatMessageAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn execute_emits_host_pinned_kind9_publish_command() {
        use nmp_core::ActorCommand;
        use std::cell::RefCell;

        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        PostChatMessageAction::execute(input(), "cid-99", &|cmd| {
            captured.borrow_mut().push(cmd);
        })
        .expect("well-formed input executes");
        let cmds = captured.into_inner();
        assert_eq!(cmds.len(), 1, "executor must send exactly one command, got {cmds:?}");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::PublishUnsignedEventToRelays { event, relays, correlation_id } => {
                assert_eq!(event.kind, KIND_CHAT_MESSAGE, "must emit kind:9");
                assert_eq!(
                    relays,
                    vec!["wss://groups.example.com".to_string()],
                    "must be pinned to the group's host relay"
                );
                assert!(
                    event.tags.iter().any(|t| t == &["h".to_string(), "room".to_string()]),
                    "must carry the ['h', local_id] group tag, got {:?}",
                    event.tags
                );
                assert_eq!(event.content, "hello");
                assert_eq!(correlation_id.as_deref(), Some("cid-99"));
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }
}
