//! In-memory caches `nmp-nip29` maintains per `moderation.md` + `routing.md`.
//!
//! ## Module layout
//!
//! - [`recent`] — `previous`-tag prefix helper (`moderation.md` §2). The recent
//!   events themselves are read from the kernel store at publish time, not from
//!   a crate-local cache.
//! - [`hosts`] — `JoinedHostsCache`: per-pubkey `(host_relay_url, local_id)`
//!   registry (`routing.md` §4.3).
//! - [`tofu`] — `TofuSignerCache` for the 39000-pinned metadata-signer trust
//!   model (`moderation.md` §4.3).
//!
//! These caches are best-effort in-memory shells. M3 LMDB persistence wires
//! them through `nmp-core::store::EventStore` once the M11.5 milestone
//! reaches Step 5; for the M11.5 Step 0 deliverable here they
//! support the routing/moderation contract tests in-memory.

mod hosts;
mod recent;
mod tofu;

pub use hosts::JoinedHostsCache;
pub use recent::{previous_tag_prefix, EventIdPrefix};
pub use tofu::{QuarantinedEvent, TofuSignerCache, TrustCheckOutcome};
