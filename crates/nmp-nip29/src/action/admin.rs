//! Admin-only actions (signer must be in latest 39001 except for `CreateGroup`,
//! which has no admin check per `kinds.md` §2.3). All emit host-pinned plans.

use nmp_core::substrate::{
    ActionContext, ActionId, ActionInput, ActionModule, ActionPlan, ActionRejection, ActionStatus,
    ActionTransition,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{
    KIND_CREATE_GROUP, KIND_CREATE_INVITE, KIND_DELETE_EVENT, KIND_DELETE_GROUP,
    KIND_EDIT_METADATA, KIND_PUT_USER, KIND_REMOVE_USER,
};

use super::publish_plan::PublishPlan;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct AdminStep;

macro_rules! admin_action {
    ($Module:ident, $Input:ident, $kind_const:expr, $build_plan:expr) => {
        #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
        pub struct $Input {
            pub group: GroupId,
            #[serde(default)]
            pub fields: ActionFields,
        }

        pub struct $Module;
        impl ActionModule for $Module {
            const NAMESPACE: &'static str = concat!("nip29.", stringify!($Module));
            type Action = $Input;
            type Step = AdminStep;
            type Output = PublishPlan;
            fn start(
                _ctx: &mut ActionContext,
                action: Self::Action,
            ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
                let plan: PublishPlan = $build_plan(&action);
                if plan.validate_no_unpinned_h().is_err() {
                    return Err(ActionRejection::Invalid(
                        "missing host pin for group event".into(),
                    ));
                }
                let _ = $kind_const; // sanity-link the constant
                Ok(ActionPlan {
                    initial_step: AdminStep,
                    initial_status: ActionStatus::Pending,
                    deadline_ms: None,
                })
            }
            fn reduce(
                _ctx: &mut ActionContext,
                _id: ActionId,
                _input: ActionInput<Self::Step>,
            ) -> ActionTransition<Self::Step, Self::Output> {
                // Step 0 deliverable: signer-bridge wiring (Steps 5/M6) flips
                // this to AwaitCapability → Complete with the signed PublishPlan.
                ActionTransition::Continue {
                    step: AdminStep,
                    status: ActionStatus::Pending,
                }
            }
        }
    };
}

/// Free-form fields shared across the admin actions; per-action validation of
/// required vs optional happens in the plan builders below.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ActionFields {
    pub target_pubkey: Option<String>,
    pub target_event_id: Option<String>,
    pub role: Option<String>,
    pub reason: Option<String>,
    pub name: Option<String>,
    pub about: Option<String>,
    pub picture: Option<String>,
    pub visibility_private: Option<bool>,
    pub access_closed: Option<bool>,
    pub restricted: Option<bool>,
    pub invite_codes: Vec<String>,
}

fn group_h_tag(group: &GroupId) -> Vec<String> {
    vec!["h".into(), group.local_id.clone()]
}

admin_action!(CreateGroupAction, CreateGroupInput, KIND_CREATE_GROUP, |a: &CreateGroupInput| {
    PublishPlan::pinned(&a.group, KIND_CREATE_GROUP, "", vec![group_h_tag(&a.group)])
});

admin_action!(EditMetadataAction, EditMetadataInput, KIND_EDIT_METADATA, |a: &EditMetadataInput| {
    let mut tags = vec![group_h_tag(&a.group)];
    if let Some(name) = &a.fields.name { tags.push(vec!["name".into(), name.clone()]); }
    if let Some(about) = &a.fields.about { tags.push(vec!["about".into(), about.clone()]); }
    if let Some(picture) = &a.fields.picture { tags.push(vec!["picture".into(), picture.clone()]); }
    if matches!(a.fields.visibility_private, Some(true)) { tags.push(vec!["private".into()]); }
    if matches!(a.fields.access_closed, Some(true)) { tags.push(vec!["closed".into()]); }
    if matches!(a.fields.restricted, Some(true)) { tags.push(vec!["restricted".into()]); }
    PublishPlan::pinned(&a.group, KIND_EDIT_METADATA, "", tags)
});

admin_action!(PutUserAction, PutUserInput, KIND_PUT_USER, |a: &PutUserInput| {
    let pubkey = a.fields.target_pubkey.clone().unwrap_or_default();
    let mut p_tag = vec!["p".into(), pubkey];
    if let Some(role) = &a.fields.role { p_tag.push(role.clone()); }
    let mut tags = vec![group_h_tag(&a.group), p_tag];
    if let Some(reason) = &a.fields.reason { tags.push(vec!["reason".into(), reason.clone()]); }
    PublishPlan::pinned(&a.group, KIND_PUT_USER, "", tags)
});

admin_action!(RemoveUserAction, RemoveUserInput, KIND_REMOVE_USER, |a: &RemoveUserInput| {
    let pubkey = a.fields.target_pubkey.clone().unwrap_or_default();
    let mut tags = vec![group_h_tag(&a.group), vec!["p".into(), pubkey]];
    if let Some(reason) = &a.fields.reason { tags.push(vec!["reason".into(), reason.clone()]); }
    PublishPlan::pinned(&a.group, KIND_REMOVE_USER, "", tags)
});

admin_action!(CreateInviteAction, CreateInviteInput, KIND_CREATE_INVITE, |a: &CreateInviteInput| {
    let mut tags = vec![group_h_tag(&a.group)];
    for code in &a.fields.invite_codes {
        tags.push(vec!["code".into(), code.clone()]);
    }
    PublishPlan::pinned(&a.group, KIND_CREATE_INVITE, "", tags)
});

admin_action!(DeleteEventAction, DeleteEventInput, KIND_DELETE_EVENT, |a: &DeleteEventInput| {
    let evt = a.fields.target_event_id.clone().unwrap_or_default();
    PublishPlan::pinned(
        &a.group,
        KIND_DELETE_EVENT,
        "",
        vec![group_h_tag(&a.group), vec!["e".into(), evt]],
    )
});

admin_action!(DeleteGroupAction, DeleteGroupInput, KIND_DELETE_GROUP, |a: &DeleteGroupInput| {
    PublishPlan::pinned(&a.group, KIND_DELETE_GROUP, "", vec![group_h_tag(&a.group)])
});
