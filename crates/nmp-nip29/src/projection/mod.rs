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

pub mod discovered;
pub mod group_chat;

pub use discovered::{DiscoveredGroup, DiscoveredGroupsProjection, DiscoveredGroupsSnapshot};
pub use group_chat::{GroupChatMessage, GroupChatProjection, GroupChatSnapshot};
