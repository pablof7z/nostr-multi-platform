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
        tags.push(vec!["e".into(), reply.clone(), "".into(), "reply".into()]);
    }
    PublishPlan::pinned(&action.group, KIND_CHAT_MESSAGE, action.content.clone(), tags)
}

pub struct PostChatMessageAction;
impl ActionModule for PostChatMessageAction {
    /// Wire-schema note: was `nip29.post_chat_message` before the namespace-prefix
    /// rename (PR-B). Every protocol crate now uses the `nmp.<nip>.<verb>`
    /// shape — enforced by doctrine-lint rule D9.
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
}
