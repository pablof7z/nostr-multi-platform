//! In-memory caches `nmp-nip29` maintains per `moderation.md` + `routing.md`.
//!
//! ## Module layout
//!
//! - [`recent`] — bounded per-group recent-events cache for `previous`-tag
//!   attachment (`moderation.md` §2.3).
//! - [`hosts`] — `JoinedHostsCache`: per-pubkey `(host_relay_url, local_id)`
//!   registry (`routing.md` §4.3).
//! - [`tofu`] — `TofuSignerCache` for the 39000-pinned metadata-signer trust
//!   model (`moderation.md` §4.3).
//!
//! These caches are best-effort in-memory shells. M3 LMDB persistence wires
//! them through `nmp-core::store::EventStore` once the M11.5 milestone
//! reaches Step 5 (Swift wiring); for the M11.5 Step 0 deliverable here they
//! support the routing/moderation contract tests in-memory.

mod hosts;
mod recent;
mod tofu;

pub use hosts::JoinedHostsCache;
pub use recent::{previous_tag_prefix, EventIdPrefix, RecentEntry, RecentGroupEvents};
pub use tofu::{QuarantinedEvent, TofuSignerCache, TrustCheckOutcome};
