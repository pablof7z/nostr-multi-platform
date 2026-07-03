//! Group-scoped native read-session composition.
//!
//! Pure NIP-29 group read doors live in `nmp_nip29::read_session`; runtime
//! hosts provide only the generic `ReadHost` seam. This native module owns the
//! remaining app-layer composition: NIP-25 reaction aggregation scoped by a
//! NIP-29 group `h` tag.

mod feed;
mod reactions;
mod types;

pub use types::{Nip25GroupReactionsHandle, Nip25GroupReactionsSession};

/// `1` = `Global` scope; group-scoped reactions pin a concrete host relay.
const SCOPE_GLOBAL: u32 = 1;

/// Snapshot key + singleton session key for the group-scoped reaction-aggregate
/// view (NIP-25 kind:7 folded by target id, scoped to one group's `h` tag).
pub const GROUP_REACTIONS_KEY: &str = "nmp.nip25.reactions";
pub(crate) const GROUP_REACTIONS_PROJECTION_TOKEN: nmp_ownership::DeclaredProjectionKey =
    nmp_ownership::DeclaredProjectionKey::framework(
        GROUP_REACTIONS_KEY,
        "projection.nmp.nip25.reactions",
    );

const GROUP_REACTIONS_CONSUMER: &str = "nip25-group-reactions";

#[cfg(test)]
#[path = "../group_feed_tests.rs"]
mod tests;
