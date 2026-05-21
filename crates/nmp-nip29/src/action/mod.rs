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
    create_group_command, create_invite_command, delete_event_command, delete_group_command,
    edit_metadata_command, put_user_command, remove_user_command, ActionFields, CreateGroupAction,
    CreateGroupInput, CreateInviteAction, CreateInviteInput, DeleteEventAction, DeleteEventInput,
    DeleteGroupAction, DeleteGroupInput, EditMetadataAction, EditMetadataInput, PutUserAction,
    PutUserInput, RemoveUserAction, RemoveUserInput,
};
pub use composed::{
    comment_in_group_command, react_in_group_command, share_event_into_group_command,
    CommentInGroupAction, CommentInGroupInput, ReactInGroupAction, ReactInGroupInput,
    ShareEventIntoGroupAction, ShareEventIntoGroupInput,
};
pub use content::{
    post_artifact_command, post_chat_message_command, post_discussion_command, PostArtifactAction,
    PostArtifactInput, PostChatMessageAction, PostChatMessageInput, PostDiscussionAction,
    PostDiscussionInput,
};
pub use membership::{
    join_request_command, leave_request_command, JoinRequestAction, JoinRequestInput,
    LeaveRequestAction, LeaveRequestInput,
};
pub use publish_plan::{PublishPlan, PublishPlanError, RelayPin};
