//! 15 `ActionModule` impls per `docs/design/nip29-crate.md` §3.3.
//!
//! Every action takes a typed `GroupId` and emits a `PublishPlan` with
//! `pin_to: Some(host_relay_url)` so the publish planner routes via the
//! third lane (Case E + Rule 9) — no NIP-65 outbox lookup, no string-typed
//! `h` tags inspected at plan time.
//!
//! ## Action layout
//!
//! - `admin` — CreateGroup, EditMetadata, PutUser, RemoveUser,
//!   CreateInvite, DeleteEvent, DeleteGroup.
//! - `membership` — JoinRequest, LeaveRequest.
//! - `content` — PostChatMessage, PostDiscussion, PostArtifact.
//! - `composed` — ShareEventIntoGroup, ReactInGroup, CommentInGroup.
//!
//! M11.5 Step 0 deliverable: trait signatures + typed inputs + correct
//! publish-plan shape. The actual wire-event construction + signer round-trip
//! lands in Step 5 alongside the Swift wiring (the actor surfaces the
//! signer capability bridge at that point).

mod admin;
mod composed;
mod content;
mod membership;
mod publish_plan;

pub use admin::{
    ActionFields, CreateGroupAction, CreateGroupInput, CreateInviteAction, CreateInviteInput,
    DeleteEventAction, DeleteEventInput, DeleteGroupAction, DeleteGroupInput,
    EditMetadataAction, EditMetadataInput, PutUserAction, PutUserInput, RemoveUserAction,
    RemoveUserInput,
};
pub use composed::{
    CommentInGroupAction, CommentInGroupInput, ReactInGroupAction, ReactInGroupInput,
    ShareEventIntoGroupAction, ShareEventIntoGroupInput,
};
pub use content::{
    PostArtifactAction, PostArtifactInput, PostChatMessageAction, PostChatMessageInput,
    PostDiscussionAction, PostDiscussionInput,
};
pub use membership::{JoinRequestAction, JoinRequestInput, LeaveRequestAction, LeaveRequestInput};
pub use publish_plan::{PublishPlan, PublishPlanError, RelayPin};

use nmp_core::substrate::ModuleRegistry;

pub fn register_all(registry: &mut ModuleRegistry) {
    registry.register_action::<CreateGroupAction>();
    registry.register_action::<JoinRequestAction>();
    registry.register_action::<LeaveRequestAction>();
    registry.register_action::<EditMetadataAction>();
    registry.register_action::<PutUserAction>();
    registry.register_action::<RemoveUserAction>();
    registry.register_action::<CreateInviteAction>();
    registry.register_action::<DeleteEventAction>();
    registry.register_action::<DeleteGroupAction>();
    registry.register_action::<PostChatMessageAction>();
    registry.register_action::<PostDiscussionAction>();
    registry.register_action::<PostArtifactAction>();
    registry.register_action::<ShareEventIntoGroupAction>();
    registry.register_action::<ReactInGroupAction>();
    registry.register_action::<CommentInGroupAction>();
}
