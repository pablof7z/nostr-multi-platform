//! User-self membership actions: `JoinRequest` (9021) and `LeaveRequest` (9022).
//!
//! Per `kinds.md` §2.2: both are signed by the prospective member / leaver, not
//! an admin. The relay reaction (auto-emit 39002, optionally consume invite
//! code) is server-side; the client just publishes the request.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{KIND_JOIN_REQUEST, KIND_LEAVE_REQUEST};

use super::publish_plan::PublishPlan;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JoinRequestInput {
    pub group: GroupId,
    #[serde(default)]
    pub invite_code: Option<String>,
    #[serde(default)]
    pub referrer_event_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

pub struct JoinRequestAction;
impl ActionModule for JoinRequestAction {
    const NAMESPACE: &'static str = "nip29.join_request";
    type Action = JoinRequestInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        let mut tags = vec![vec!["h".into(), action.group.local_id.clone()]];
        if let Some(code) = action.invite_code {
            tags.push(vec!["code".into(), code]);
        }
        if let Some(ref evt) = action.referrer_event_id {
            tags.push(vec!["e".into(), evt.clone()]);
        }
        let content = action.reason.unwrap_or_default();
        let plan = PublishPlan::pinned(&action.group, KIND_JOIN_REQUEST, content, tags);
        plan.validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for join request".into()))?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LeaveRequestInput {
    pub group: GroupId,
    #[serde(default)]
    pub reason: Option<String>,
}

pub struct LeaveRequestAction;
impl ActionModule for LeaveRequestAction {
    const NAMESPACE: &'static str = "nip29.leave_request";
    type Action = LeaveRequestInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        let tags = vec![vec!["h".into(), action.group.local_id.clone()]];
        let plan = PublishPlan::pinned(
            &action.group,
            KIND_LEAVE_REQUEST,
            action.reason.unwrap_or_default(),
            tags,
        );
        plan.validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for leave request".into()))?;
        Ok(())
    }
}
