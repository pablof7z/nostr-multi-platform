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
//! - `create` — `CreatePublicGroup` (kind:9007 + kind:9002).
//! - `discover` — `DiscoverGroups` (no publish; pushes a metadata interest).
//! - `join` — `JoinGroup` (kind:9021, user-management request).
//!
//! NIP-29 ships its public group creation, relay-group chat surface,
//! discovery, and join in v1. The remaining 9000-9009 admin actions stay
//! out of scope for this milestone — Marmot MLS covers private groups.

mod composed;
mod content;
mod create;
mod discover;
mod join;
mod publish_plan;

pub use composed::{ReactInGroupAction, ReactInGroupInput};
pub use content::{PostChatMessageAction, PostChatMessageInput};
pub use create::{CreatePublicGroupAction, CreatePublicGroupInput};
pub use discover::{DiscoverGroupsAction, DiscoverGroupsInput};
pub use join::{JoinGroupAction, JoinGroupInput};
pub use publish_plan::{PublishPlan, PublishPlanError, RelayPin};
