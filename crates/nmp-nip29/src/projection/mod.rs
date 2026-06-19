//! Read-side projections for NIP-29 groups.
//!
//! Each submodule owns one screen's read model. They share the same wiring
//! shape — a [`nmp_core::KernelEventObserver`] for ingest plus a no-argument
//! `snapshot_json` for `nmp_core::NmpApp::register_snapshot_projection`:
//!
//! - [`group_chat`] — [`GroupChatProjection`]: one group's chat-content
//!   events (kinds 9/11) keyed by `["h", local_id]`. The read-side
//!   of `GroupChatView`.
//! - [`discovered`] — [`DiscoveredGroupsProjection`]: a single relay's
//!   group catalog, accumulated from kinds 39000/39001/39002. The read-side
//!   of `JoinGroupView` / discovery flows.
//! - [`group_defaults`] — [`GroupDefaultsProjection`]: crate-owned defaults for
//!   the public-group create flow (the suggested relay URL, #626). Output-only:
//!   not a `KernelEventObserver` — its snapshot is a pure function of a
//!   crate-owned constant.
//! - [`joined`] — [`JoinedGroupsProjection`]: active-account membership/admin
//!   status derived from relay-signed 39001/39002 snapshots.

pub mod discovered;
pub mod group_chat;
pub mod group_defaults;
pub mod joined;

pub use discovered::{DiscoveredGroup, DiscoveredGroupsProjection, DiscoveredGroupsSnapshot};
pub use group_chat::{GroupChatMessage, GroupChatProjection, GroupChatSnapshot};
pub use group_defaults::{
    GroupDefaultsProjection, GroupDefaultsSnapshot, DEFAULT_PUBLIC_GROUP_RELAY_URL,
};
pub use joined::{JoinedGroup, JoinedGroupsProjection, JoinedGroupsSnapshot};
