//! 7 `ActionModule` impls per `docs/plan/marmot-mls.md` §Step 2.
//!
//! `PublishKeyPackage`, `CreateGroup`, `InviteMember`, `SendMessage`,
//! `LeaveGroup`, `RemoveMember`, `UpdateKeys`.
//!
//! Each action takes a typed group identity and emits a [`PublishPlan`] with
//! the relay pin set (kind:445 group events) or unset (kind:30443/443
//! KeyPackages → standard author-write outbox), so the publish planner routes
//! via the typed carrier and never inspects raw tags. The actual MDK-driven
//! event production lives in [`crate::service`] (mirrors `nmp-nip29`'s
//! two-layer Step-0 split — these impls import ZERO MDK types).

mod actions;
mod publish_plan;

pub use actions::{
    CreateGroupAction, GroupActionInput, InviteMemberAction, LeaveGroupAction, MarmotActionFields,
    PublishKeyPackageAction, PublishKeyPackageInput, RemoveMemberAction, SendMessageAction,
    UpdateKeysAction,
};
pub use publish_plan::{PublishPlan, PublishPlanError, RelayPin};
