//! The 3 group-chat `ActionModule` impls a `GroupChatView` consumes.
//!
//! Every action takes a typed `GroupId` and emits a `PublishPlan` with
//! `pin_to: Some(host_relay_url)` so the publish planner routes via the
//! third lane (Case E + Rule 9) — no NIP-65 outbox lookup, no string-typed
//! `h` tags inspected at plan time.
//!
//! ## Action layout
//!
//! - `content` — PostChatMessage (kind:9).
//! - `composed` — ReactInGroup (kind:7+h), CommentInGroup (kind:1111+h).
//!
//! The admin / membership / artifact / discussion / share executors were
//! deleted: NIP-29 ships only its relay-group chat surface in v1 (no group
//! administration UI is planned — Marmot MLS covers private groups).

mod composed;
mod content;
mod publish_plan;

pub use composed::{
    comment_in_group_command, react_in_group_command, CommentInGroupAction, CommentInGroupInput,
    ReactInGroupAction, ReactInGroupInput,
};
pub use content::{post_chat_message_command, PostChatMessageAction, PostChatMessageInput};
pub use publish_plan::{PublishPlan, PublishPlanError, RelayPin};
