//! The relay-group chat `ActionModule` impls a `GroupChatView` consumes.
//!
//! Every action takes a typed `GroupId` and emits a `PublishPlan` with
//! `pin_to: Some(host_relay_url)` so the publish planner routes via the
//! third lane (Case E + Rule 9) — no NIP-65 outbox lookup, no string-typed
//! `h` tags inspected at plan time.
//!
//! ## Action layout
//!
//! - `publish` — `PublishGroupEvent`: the SOLE write surface (any kind; injects
//!   the `h` / `previous` / pin envelope onto a caller-supplied event). NIP-29
//!   owns the envelope, not the event kind — "chat" is just `kind:9`. Per-kind
//!   event construction lives in the owning NIP crate (kind:7 reactions and the
//!   kind:5 retraction in `nmp-nip25`, kind:16 reposts in `nmp-nip18`,
//!   kind:11/other content in the app/component layer): each builds the event,
//!   then hands it to this generic surface (#2513, codifying #2504/#2505).
//! - `create` — `CreatePublicGroup` (kind:9007 + kind:9002).
//! - `discover` — `DiscoverGroups` (no publish; pushes a metadata interest).
//! - `join` — `JoinGroup` (kind:9021, user-management request).
//! - `leave` — `LeaveGroup` (kind:9022, user-management request).
//! - `admin` — `PutUser` (kind:9000) and `CreateInvite` (kind:9009).
//! - `set_parent` — `SetParent` (kind:9002 edit-metadata, NIP-29 subgroups #2319).
//! - `edit_metadata` — `EditMetadata` (kind:9002 edit-metadata): edit an
//!   existing group's name/about/picture/visibility/access (admin action).
//!
//! NIP-29 ships public group creation, generic group-event publishing,
//! discovery, join, and the ADR-0060 admin subset (`9000` / `9009`) in v1. The
//! other moderation actions remain out of this increment.

mod admin;
mod create;
mod discover;
mod edit_metadata;
mod join;
mod leave;
mod metadata_tags;
mod publish;
mod publish_plan;
mod set_parent;

pub use admin::{
    CreateInviteAction, CreateInviteInput, PutUserAction, PutUserInput, MAX_CODES_PER_INVITE_EVENT,
};
pub use publish::{PublishGroupEventAction, PublishGroupEventInput, DEFAULT_PREVIOUS_LIMIT};
pub use create::{CreatePublicGroupAction, CreatePublicGroupInput, GroupAccess, GroupVisibility};
pub use discover::{DiscoverGroupsAction, DiscoverGroupsInput};
pub use edit_metadata::{EditMetadataAction, EditMetadataInput};
pub use join::{JoinGroupAction, JoinGroupInput};
pub use leave::{LeaveGroupAction, LeaveGroupInput};
pub use publish_plan::{PublishPlan, PublishPlanError, RelayPin};
pub use set_parent::{SetParentAction, SetParentInput};
