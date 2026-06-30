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
//! - [`action`] — the group `ActionModule` impls, fronted by the generic
//!   `PublishGroupEvent` (publish any event to a group), plus react / share /
//!   repost conveniences and the lifecycle/admin actions.
//! - [`cache`] — `previous_tag_prefix` helper, `JoinedHostsCache`,
//!   `TofuSignerCache` (metadata-signer trust).
//! - [`interest`] — helpers for constructing pinned `LogicalInterest`s.
//! - [`projection`] — `GroupEventsProjection`: the read-side of a group-chat
//!   screen, plus raw group-event projections for reusable `h`-tag mechanics.
//!
//! All inputs to actions carry a typed `GroupId` so the publish planner gets a
//! typed `PublishPlan::pin_to: Some(host)` carrier and never derives routing
//! from raw tag strings.
//!
//! read-side extension path is `projection::GroupEventsProjection` via
//! `ObservedProjectionSink` — see `nmp_core::substrate` module docs.

pub mod action;
pub mod cache;
pub mod group_id;
pub mod group_query;
pub mod input_scope;
pub mod interest;
pub mod kinds;
pub mod projection;
pub mod register;
pub mod reply;
pub mod search;
pub mod wire;

pub use group_id::GroupId;
pub use group_query::{GroupEventKinds, GroupEventsQuery};
pub use input_scope::{
    register_input_scopes, GroupIdentPayload, GroupInputScopeRecognizer, GROUP_INPUT_SCOPE_LABEL,
};
pub use kinds::{event_is_group_event, group_id_from_tags, GroupEventClass, KindClass};
pub use projection::{
    DiscoveredGroup, DiscoveredGroupsProjection, DiscoveredGroupsSnapshot, GroupEvent,
    GroupEventsProjection, GroupEventsSnapshot, GroupDefaultsProjection, GroupDefaultsSnapshot,
    GroupRole, GroupRosterMember, GroupRosterProjection, GroupRosterSnapshot, JoinedGroup,
    JoinedGroupsProjection, JoinedGroupsSnapshot, DEFAULT_PUBLIC_GROUP_RELAY_URL,
};
pub use register::register_actions;
pub use search::{register_search_scopes, GroupMetadataSearchScope, GROUP_SEARCH_SCOPE_LABEL};
pub use wire::discovered_groups_fb::{
    decode_discovered_groups_snapshot, encode_discovered_groups_snapshot,
    DISCOVERED_GROUPS_FILE_IDENTIFIER, DISCOVERED_GROUPS_SCHEMA_ID,
    DISCOVERED_GROUPS_SCHEMA_VERSION,
};
pub use wire::group_events_fb::{
    decode_group_events_snapshot, encode_group_events_snapshot, GROUP_EVENTS_FILE_IDENTIFIER,
    GROUP_EVENTS_SCHEMA_ID, GROUP_EVENTS_SCHEMA_VERSION,
};
pub use wire::group_defaults_fb::{
    decode_group_defaults_snapshot, encode_group_defaults_snapshot, GROUP_DEFAULTS_FILE_IDENTIFIER,
    GROUP_DEFAULTS_SCHEMA_ID, GROUP_DEFAULTS_SCHEMA_VERSION,
};
pub use wire::group_roster_fb::{
    decode_group_roster_snapshot, encode_group_roster_snapshot, GROUP_ROSTER_FILE_IDENTIFIER,
    GROUP_ROSTER_SCHEMA_ID, GROUP_ROSTER_SCHEMA_VERSION,
};
pub use wire::joined_groups_fb::{
    decode_joined_groups_snapshot, encode_joined_groups_snapshot, JOINED_GROUPS_FILE_IDENTIFIER,
    JOINED_GROUPS_SCHEMA_ID, JOINED_GROUPS_SCHEMA_VERSION,
};

#[cfg(test)]
mod tests;
