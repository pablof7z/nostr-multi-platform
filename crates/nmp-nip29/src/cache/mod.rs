//! Caches `nmp-nip29` maintains per `moderation.md` + `routing.md`.
//!
//! ## Module layout
//!
//! - [`recent`] — `previous`-tag prefix helper (`moderation.md` §2). The recent
//!   events themselves are read from the kernel store at publish time, not from
//!   a crate-local cache.
//! - [`hosts`] — `JoinedHostsCache`: per-pubkey `(host_relay_url, local_id)`
//!   registry (`routing.md` §4.3). Durable via `EventStore` domain table
//!   (`nmp.nip29.joined_hosts`) — open with [`JoinedHostsCache::open`].
//! - [`tofu`] — `TofuSignerCache` for the 39000-pinned metadata-signer trust
//!   model (`moderation.md` §4.3). Durable via `EventStore` domain table
//!   (`nmp.nip29.tofu_signer`) — open with [`TofuSignerCache::open`].
//!
//! Both caches expose a `new()` constructor for pure in-memory use (tests /
//! no-store contexts) and an `open(store)` constructor that loads state from
//! the durable store on startup and writes through on every mutation (D4:
//! single-writer, persist-before-memory update). Persistence fixes #2286.

mod hosts;
mod recent;
mod tofu;

pub use hosts::JoinedHostsCache;
pub use recent::{previous_tag_prefix, EventIdPrefix};
pub use tofu::{QuarantinedEvent, TofuSignerCache, TrustCheckOutcome};
