//! User-sent content actions: chat (9), discussion (11 + t=discussion),
//! artifact (11 + catalog tags).

use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPlan, ActionRejection, ActionStatus,
};
use serde::{Deserialize, Serialize};

use crate::cache::previous_tag_prefix;
use crate::group_id::GroupId;
use crate::kinds::{KIND_CHAT_MESSAGE, KIND_DISCUSSION_OR_ARTIFACT};

use super::publish_plan::PublishPlan;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ContentStep;

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

pub struct PostChatMessageAction;
impl ActionModule for PostChatMessageAction {
    const NAMESPACE: &'static str = "nip29.post_chat_message";
    type Action = PostChatMessageInput;
    type Step = ContentStep;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        if action.content.is_empty() {
            return Err(ActionRejection::Invalid("empty chat message".into()));
        }
        let mut tags = vec![vec!["h".into(), action.group.local_id.clone()]];
        for prefix in &action.previous_event_id_prefixes {
            tags.push(vec!["previous".into(), previous_tag_prefix(prefix)]);
        }
        if let Some(reply) = &action.reply_to_event_id {
            tags.push(vec!["e".into(), reply.clone(), "".into(), "reply".into()]);
        }
        let plan = PublishPlan::pinned(&action.group, KIND_CHAT_MESSAGE, action.content, tags);
        plan.validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for chat message".into()))?;
        Ok(ActionPlan {
            initial_step: ContentStep,
            initial_status: ActionStatus::Pending,
            deadline_ms: None,
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PostDiscussionInput {
    pub group: GroupId,
    pub title: Option<String>,
    pub body: String,
    #[serde(default)]
    pub image_urls: Vec<String>,
}

pub struct PostDiscussionAction;
impl ActionModule for PostDiscussionAction {
    const NAMESPACE: &'static str = "nip29.post_discussion";
    type Action = PostDiscussionInput;
    type Step = ContentStep;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        let mut tags = vec![
            vec!["h".into(), action.group.local_id.clone()],
            vec!["t".into(), "discussion".into()],
        ];
        if let Some(title) = &action.title { tags.push(vec!["title".into(), title.clone()]); }
        for img in &action.image_urls { tags.push(vec!["image".into(), img.clone()]); }
        let plan = PublishPlan::pinned(
            &action.group,
            KIND_DISCUSSION_OR_ARTIFACT,
            action.body,
            tags,
        );
        plan.validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for discussion".into()))?;
        Ok(ActionPlan {
            initial_step: ContentStep,
            initial_status: ActionStatus::Pending,
            deadline_ms: None,
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PostArtifactInput {
    pub group: GroupId,
    pub artifact_id: String,
    pub url_reference: Option<String>,
    pub isbn_reference: Option<String>,
    pub naddr_reference: Option<String>,
    pub title: Option<String>,
    pub note: String,
}

pub struct PostArtifactAction;
impl ActionModule for PostArtifactAction {
    const NAMESPACE: &'static str = "nip29.post_artifact";
    type Action = PostArtifactInput;
    type Step = ContentStep;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        let mut tags = vec![
            vec!["h".into(), action.group.local_id.clone()],
            vec!["d".into(), action.artifact_id.clone()],
        ];
        if let Some(u) = &action.url_reference { tags.push(vec!["r".into(), u.clone()]); }
        if let Some(i) = &action.isbn_reference { tags.push(vec!["i".into(), i.clone()]); }
        if let Some(a) = &action.naddr_reference { tags.push(vec!["a".into(), a.clone()]); }
        if let Some(t) = &action.title { tags.push(vec!["title".into(), t.clone()]); }
        let plan = PublishPlan::pinned(
            &action.group,
            KIND_DISCUSSION_OR_ARTIFACT,
            action.note,
            tags,
        );
        plan.validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for artifact share".into()))?;
        Ok(ActionPlan {
            initial_step: ContentStep,
            initial_status: ActionStatus::Pending,
            deadline_ms: None,
        })
    }
}
