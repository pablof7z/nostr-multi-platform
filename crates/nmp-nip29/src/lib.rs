//! `nmp-nip29` — NIP-29 relay-based groups as an NMP protocol crate.
//!
//! Implements the design in `docs/design/nip29-crate.md` + the three sub-docs:
//! - `docs/design/nip29/routing.md` (host-relay-pin contract; lattice Rule 9)
//! - `docs/design/nip29/kinds.md` (event-kind catalog; 39000–39003 metadata,
//!   9000–9022 moderation, h-tagged user-sent group events)
//! - `docs/design/nip29/moderation.md` (TOFU + NIP-11-strict trust model,
//!   `previous`-tag chain, audit-only mutation policy)
//!
//! ## Crate boundary (M11.5 exit gate)
//!
//! - `nmp-nip29` does NOT import any other `nmp-nip*` crate. Cross-protocol
//!   composition happens at the app layer.
//! - `nmp-core` gains zero group / community / room nouns; this crate owns
//!   them. The only generic surface added in `nmp-core` is the third routing
//!   lane (`InterestShape::relay_pin` + lattice Rule 9 + partition Case E).
//!
//! ## Module layout
//!
//! - [`group_id`] — `GroupId { host_relay_url, local_id }` + URI codec.
//! - [`kinds`] — NIP-29 kind constants and the `["h", ...]` dispatch helper.
//! - [`action`] — the 3 group-chat `ActionModule` impls (post chat message,
//!   react, comment).
//! - [`cache`] — `RecentGroupEvents` (previous-tag), `JoinedHostsCache`,
//!   `TofuSignerCache` (metadata-signer trust).
//! - [`interest`] — helpers for constructing pinned `LogicalInterest`s.
//! - [`projection`] — `GroupChatProjection`: the read-side of a group-chat
//!   screen (a `KernelEventObserver` projecting kind 9/11/1111 events).
//!
//! All inputs to actions carry a typed `GroupId` so the publish planner gets a
//! typed `PublishPlan::pin_to: Some(host)` carrier and never derives routing
//! from raw tag strings.
//!
//! The former `domain` (13 per-kind domain modules) and `view` (7 reactive
//! views) modules were deleted: they had zero non-test consumers. The live
//! read-side extension path is `projection::GroupChatProjection` via
//! `KernelEventObserver` — see `nmp_core::substrate` module docs.

pub mod action;
pub mod cache;
pub mod group_id;
pub mod interest;
pub mod kinds;
pub mod projection;
pub mod register;

pub use group_id::GroupId;
pub use kinds::{event_is_group_event, group_id_from_tags, GroupEventClass, KindClass};
pub use projection::{
    DiscoveredGroup, DiscoveredGroupsProjection, DiscoveredGroupsSnapshot, GroupChatMessage,
    GroupChatProjection, GroupChatSnapshot,
};

#[cfg(test)]
mod tests;
