//! The 3 group-chat `ActionModule` impls a `GroupChatView` consumes.
//!
//! Every action takes a typed `GroupId` and emits a `PublishPlan` with
//! `pin_to: Some(host_relay_url)` so the publish planner routes via the
//! third lane (Case E + Rule 9) — no NIP-65 outbox lookup, no string-typed
//! `h` tags inspected at plan time.
//!
//! ## Action layout
//!
//! - `content` — `PostChatMessage` (kind:9).
//! - `composed` — `ReactInGroup` (kind:7+h), `CommentInGroup` (kind:1111+h).
//! - `discover` — `DiscoverGroups` (no publish; pushes a metadata interest).
//! - `join` — `JoinGroup` (kind:9021, user-management request).
//!
//! NIP-29 ships its relay-group chat surface plus discovery + join in v1.
//! Group administration (9000-9009 admin actions) remains out of scope for
//! this crate — Marmot MLS covers private groups.

mod composed;
mod content;
mod discover;
mod join;
mod publish_plan;

pub use composed::{
    CommentInGroupAction, CommentInGroupInput, ReactInGroupAction, ReactInGroupInput,
};
pub use content::{PostChatMessageAction, PostChatMessageInput};
pub use discover::{DiscoverGroupsAction, DiscoverGroupsInput};
pub use join::{JoinGroupAction, JoinGroupInput};
pub use publish_plan::{PublishPlan, PublishPlanError, RelayPin};
