//! `ShareEventIntoGroup` (kind:16), `ReactInGroup` (kind:7+h),
//! `CommentInGroup` (kind:1111+h).
//!
//! These are the "host-pinned variant of an otherwise cross-protocol action"
//! per `kinds.md` §4. They live here because the routing concern (the `h`
//! tag) is the discriminator; the corresponding non-`h` actions live in
//! `nmp-nip25` / `nmp-nip22` / future `nmp-nip18`.

use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPlan, ActionRejection, ActionStatus,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{KIND_COMMENT, KIND_REACTION, KIND_REPOST};

use super::publish_plan::PublishPlan;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ComposedStep;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ShareEventIntoGroupInput {
    pub group: GroupId,
    pub event_ref: String,
    pub original_author_pubkey: Option<String>,
    pub original_kind: Option<u32>,
}

pub struct ShareEventIntoGroupAction;
impl ActionModule for ShareEventIntoGroupAction {
    const NAMESPACE: &'static str = "nip29.share_event_into_group";
    type Action = ShareEventIntoGroupInput;
    type Step = ComposedStep;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        let mut tags = vec![
            vec!["h".into(), action.group.local_id.clone()],
            vec!["e".into(), action.event_ref.clone()],
        ];
        if let Some(p) = &action.original_author_pubkey {
            tags.push(vec!["p".into(), p.clone()]);
        }
        if let Some(k) = action.original_kind {
            tags.push(vec!["k".into(), k.to_string()]);
        }
        let plan = PublishPlan::pinned(&action.group, KIND_REPOST, "", tags);
        plan.validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for share-into-group".into()))?;
        Ok(ActionPlan { initial_step: ComposedStep, initial_status: ActionStatus::Pending, deadline_ms: None })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReactInGroupInput {
    pub group: GroupId,
    pub target_event_id: String,
    pub target_author_pubkey: Option<String>,
    pub content: String,
}

pub struct ReactInGroupAction;
impl ActionModule for ReactInGroupAction {
    const NAMESPACE: &'static str = "nip29.react_in_group";
    type Action = ReactInGroupInput;
    type Step = ComposedStep;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        let mut tags = vec![
            vec!["h".into(), action.group.local_id.clone()],
            vec!["e".into(), action.target_event_id.clone()],
        ];
        if let Some(p) = &action.target_author_pubkey {
            tags.push(vec!["p".into(), p.clone()]);
        }
        let plan = PublishPlan::pinned(&action.group, KIND_REACTION, action.content, tags);
        plan.validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for in-group reaction".into()))?;
        Ok(ActionPlan { initial_step: ComposedStep, initial_status: ActionStatus::Pending, deadline_ms: None })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CommentInGroupInput {
    pub group: GroupId,
    pub root_event_id: Option<String>,
    pub parent_event_id: Option<String>,
    pub content: String,
}

pub struct CommentInGroupAction;
impl ActionModule for CommentInGroupAction {
    const NAMESPACE: &'static str = "nip29.comment_in_group";
    type Action = CommentInGroupInput;
    type Step = ComposedStep;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        let mut tags = vec![vec!["h".into(), action.group.local_id.clone()]];
        if let Some(root) = &action.root_event_id {
            tags.push(vec!["E".into(), root.clone()]);
        }
        if let Some(parent) = &action.parent_event_id {
            tags.push(vec!["e".into(), parent.clone()]);
        }
        let plan = PublishPlan::pinned(&action.group, KIND_COMMENT, action.content, tags);
        plan.validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for in-group comment".into()))?;
        Ok(ActionPlan { initial_step: ComposedStep, initial_status: ActionStatus::Pending, deadline_ms: None })
    }
}
