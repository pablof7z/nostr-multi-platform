//! `ReactInGroup` (kind:7+h), `CommentInGroup` (kind:1111+h).
//!
//! These are the "host-pinned variant of an otherwise cross-protocol action"
//! per `kinds.md` §4. They live here because the routing concern (the `h`
//! tag) is the discriminator; the corresponding non-`h` actions live in
//! `nmp-nip25` / `nmp-nip22`.

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
    /// Wire-schema note: was `nip29.react_in_group` before the namespace-prefix
    /// rename (PR-B). Every protocol crate now uses the `nmp.<nip>.<verb>`
    /// shape — enforced by doctrine-lint rule D9.
    const NAMESPACE: &'static str = "nmp.nip29.react_in_group";
    type Action = ReactInGroupInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
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
    /// Wire-schema note: was `nip29.comment_in_group` before the namespace-prefix
    /// rename (PR-B). Every protocol crate now uses the `nmp.<nip>.<verb>`
    /// shape — enforced by doctrine-lint rule D9.
    const NAMESPACE: &'static str = "nmp.nip29.comment_in_group";
    type Action = CommentInGroupInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
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
