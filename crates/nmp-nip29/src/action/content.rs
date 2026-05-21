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

/// Map a validated `nmp.nip29.post_chat_message` action JSON to the [`ActorCommand`]
/// that publishes the kind:9 group chat message.
pub fn post_chat_message_command(action_json: &str) -> Result<ActorCommand, String> {
    let input: PostChatMessageInput =
        serde_json::from_str(action_json).map_err(|e| e.to_string())?;
    post_chat_message_plan(&input).into_actor_command()
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
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(post_chat_message_plan(&action).into_actor_command()?);
        Ok(())
    }
}
