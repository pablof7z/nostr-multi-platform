//! The relay-group chat `ActionModule` impls a `GroupChatView` consumes.
//!
//! Every action takes a typed `GroupId` and emits a `PublishPlan` with
//! `pin_to: Some(host_relay_url)` so the publish planner routes via the
//! third lane (Case E + Rule 9) — no NIP-65 outbox lookup, no string-typed
//! `h` tags inspected at plan time.
//!
//! ## Action layout
//!
//! - `content` — `PostChatMessage` (kind:9).
//! - `composed` — `ReactInGroup` (kind:7+h).
//! - `group_event` — raw share/repost-in-group producers (kind:11/16+h).
//! - `create` — `CreatePublicGroup` (kind:9007 + kind:9002).
//! - `discover` — `DiscoverGroups` (no publish; pushes a metadata interest).
//! - `join` — `JoinGroup` (kind:9021, user-management request).
//! - `admin` — `PutUser` (kind:9000) and `CreateInvite` (kind:9009).
//!
//! NIP-29 ships public group creation, relay-group chat, discovery, join, and
//! the ADR-0060 admin subset (`9000` / `9009`) in v1. The other moderation
//! actions remain out of this increment.

mod admin;
mod composed;
mod content;
mod create;
mod discover;
mod group_event;
mod join;
mod publish_plan;

pub use admin::{
    CreateInviteAction, CreateInviteInput, PutUserAction, PutUserInput, MAX_CODES_PER_INVITE_EVENT,
};
pub use composed::{ReactInGroupAction, ReactInGroupInput};
pub use content::{PostChatMessageAction, PostChatMessageInput};
pub use create::{CreatePublicGroupAction, CreatePublicGroupInput};
pub use discover::{DiscoverGroupsAction, DiscoverGroupsInput};
pub use group_event::{
    GroupEventTarget, RepostInGroupAction, RepostInGroupInput, ShareEventInGroupAction,
    ShareEventInGroupInput,
};
pub use join::{JoinGroupAction, JoinGroupInput};
pub use publish_plan::{PublishPlan, PublishPlanError, RelayPin};
