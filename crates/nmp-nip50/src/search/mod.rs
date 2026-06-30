//! Higher-order NIP-50 search orchestration owned by `nmp-nip50`.
//!
//! The substrate (`nmp-core` / `nmp-planner`) owns only the generic
//! `InterestShape.search` wire field, generic ingest/observer fan-out, and the
//! blocked-relay subtractive post-pass. THIS module owns NIP-50 relay selection
//! ([`relays`]) and the per-relay relay-pinned interest plan ([`plan`]). The
//! deduplicating result projection itself lives in the crate-root `projection`
//! module (issue #1811 / cache FTS).
//!
//! ## Where `open_search` lives
//!
//! The pure, host-agnostic primitives live here so the crate-registered scope
//! registry (#1811) can plug a new [`crate::SearchScope`] in without touching
//! the host. The host-driving entrypoint (`NmpApp::open_search` / `_close` /
//! `_snapshot` and the C-ABI symbols) lives in `nmp-ffi`, the composition root
//! that owns the `NmpApp` actor handle. `nmp-ffi` depends on `nmp-nip50`;
//! `nmp-nip50` never names `nmp-ffi` (D0 acyclic).

pub mod plan;
pub mod relays;
pub mod session;

pub use plan::{search_relay_plan, RelayPinnedInterest};
pub use relays::{
    install_search_relay_source, resolve_search_relays, SearchFallbackRelays, SearchRelaySource,
};
pub use session::{SearchSessionBuild, SearchSessionRegistry, SearchTeardownAction};
