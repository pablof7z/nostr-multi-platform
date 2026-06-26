//! The relay-group chat `ActionModule` impls a `GroupChatView` consumes.
//!
//! Every action takes a typed `GroupId` and emits a `PublishPlan` with
//! `pin_to: Some(host_relay_url)` so the publish planner routes via the
//! third lane (Case E + Rule 9) — no NIP-65 outbox lookup, no string-typed
//! `h` tags inspected at plan time.
//!
//! ## Action layout
//!
//! - `publish` — `PublishGroupEvent`: the generic "publish this event to group X"
//!   surface (any kind; injects the `h` / `previous` / pin envelope). NIP-29
//!   owns the envelope, not the event kind — "chat" is just `kind:9`.
//! - `composed` — `ReactInGroup` (kind:7+h): thin convenience over `publish`.
//! - `group_event` — share/repost-in-group producers (kind:11/16+h): thin
//!   convenience over `publish`.
//! - `create` — `CreatePublicGroup` (kind:9007 + kind:9002).
//! - `discover` — `DiscoverGroups` (no publish; pushes a metadata interest).
//! - `join` — `JoinGroup` (kind:9021, user-management request).
//! - `leave` — `LeaveGroup` (kind:9022, user-management request).
//! - `admin` — `PutUser` (kind:9000) and `CreateInvite` (kind:9009).
//! - `set_parent` — `SetParent` (kind:9002 edit-metadata, NIP-29 subgroups #2319).
//!
//! NIP-29 ships public group creation, generic group-event publishing,
//! discovery, join, and the ADR-0060 admin subset (`9000` / `9009`) in v1. The
//! other moderation actions remain out of this increment.

mod admin;
mod composed;
mod create;
mod discover;
mod group_event;
mod join;
mod leave;
mod metadata_tags;
mod publish;
mod publish_plan;
mod set_parent;

pub use admin::{
    CreateInviteAction, CreateInviteInput, PutUserAction, PutUserInput, MAX_CODES_PER_INVITE_EVENT,
};
pub use composed::{ReactInGroupAction, ReactInGroupInput};
pub use publish::{PublishGroupEventAction, PublishGroupEventInput, DEFAULT_PREVIOUS_LIMIT};
pub use create::{CreatePublicGroupAction, CreatePublicGroupInput, GroupAccess, GroupVisibility};
pub use discover::{DiscoverGroupsAction, DiscoverGroupsInput};
pub use group_event::{
    GroupEventTarget, RepostInGroupAction, RepostInGroupInput, ShareEventInGroupAction,
    ShareEventInGroupInput,
};
pub use join::{JoinGroupAction, JoinGroupInput};
pub use leave::{LeaveGroupAction, LeaveGroupInput};
pub use publish_plan::{PublishPlan, PublishPlanError, RelayPin};
pub use set_parent::{SetParentAction, SetParentInput};
