//! Higher-order NIP-50 search orchestration owned by `nmp-nip50`.
//!
//! The substrate (`nmp-core` / `nmp-planner`) owns only the generic
//! `InterestShape.search` wire field, generic ingest/observer fan-out, and the
//! blocked-relay subtractive post-pass. THIS module owns NIP-50 relay selection
//! ([`relays`]) and the per-relay relay-pinned interest plan ([`plan`]). The
//! deduplicating result projection itself lives in the crate-root `projection`
//! module (issue #1811 / cache FTS).
//!
//! ## Where `open_search_read` lives
//!
//! The public search doorway lives here in the concept crate. Runtime hosts
//! implement `nmp-read-session::ReadHost` once, then call this crate-owned door
//! with their relay-source and store capabilities. A host that does not import
//! `nmp-nip50` has no search symbol.

pub mod plan;
pub mod relays;
pub mod session;

pub use plan::{search_relay_plan, RelayPinnedInterest};
pub use relays::{
    install_search_relay_source, resolve_search_relays, SearchFallbackRelays, SearchRelaySource,
};
pub use session::{
    close_search_read, close_search_read_by_key, open_search_read, search_consumer,
    search_projection_key, OpenSearchRead, SearchReadHandle,
};
