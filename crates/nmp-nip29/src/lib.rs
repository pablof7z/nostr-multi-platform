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
//!   composition happens at the app layer (e.g. `nmp-highlighter-core`).
//! - `nmp-core` gains zero group / community / room nouns; this crate owns
//!   them. The only generic surface added in `nmp-core` is the third routing
//!   lane (`InterestShape::relay_pin` + lattice Rule 9 + partition Case E).
//!
//! ## Module layout
//!
//! - [`group_id`] — `GroupId { host_relay_url, local_id }` + URI codec.
//! - [`kinds`] — NIP-29 kind constants and the `["h", ...]` dispatch helper.
//! - [`domain`] — 13 `DomainModule` impls.
//! - [`view`] — 7 `ViewModule` impls.
//! - [`action`] — 15 `ActionModule` impls.
//! - [`cache`] — `RecentGroupEvents` (previous-tag), `JoinedHostsCache`,
//!   `TofuSignerCache` (metadata-signer trust).
//! - [`interest`] — helpers for constructing pinned `LogicalInterest`s.
//! - [`moderation`] — audit-record materialisation; canonical-state separation.
//!
//! All inputs to actions carry a typed `GroupId` so the publish planner gets a
//! typed `relay_pin: Some(host)` carrier and never derives routing from raw tag
//! strings.

pub mod cache;
pub mod domain;
pub mod group_id;
pub mod interest;
pub mod kinds;
pub mod moderation;
pub mod view;

pub mod action;

pub use group_id::GroupId;
pub use kinds::{event_is_group_event, group_id_from_tags, GroupEventClass, KindClass};

/// Register every module produced by `nmp-nip29` into a kernel
/// `ModuleRegistry`. Called by per-app generated code (`nmp-codegen`) at
/// startup so the kernel knows the 13/7/15 trait-family populations exist.
pub fn register(registry: &mut nmp_core::substrate::ModuleRegistry) {
    domain::register_all(registry);
    view::register_all(registry);
    action::register_all(registry);
}

#[cfg(test)]
mod tests;
