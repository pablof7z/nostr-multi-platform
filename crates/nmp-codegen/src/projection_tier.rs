//! ADR-0070 / Workstream-E4 — projection-tier classification.
//!
//! #1723 (epic #1719): the tier classification is no longer hand-maintained
//! here. The neutral [`crate::projection_contract`] manifest owns each
//! projection's [`ProjectionTier`]; this module is now a thin lookup that reads
//! the tier off the contract (fail-closed) so the existing
//! `projection_tier(json_key)` import path keeps working without a parallel
//! classification table. The derived kernel built-in key set lives on the
//! contract too ([`crate::projection_contract::kernel_builtin_projection_keys`]),
//! re-exported through this module's old public path.

pub use crate::projection_contract::{kernel_builtin_projection_keys, ProjectionTier};

/// Classify a projection key into its [`ProjectionTier`] by reading the neutral
/// projection contract. Fail-closed: a key with no contract entry panics (a new
/// projection without a contract row is a programming error), so the
/// classification cannot silently drift from the manifest.
///
/// # Panics
/// When `key` has no [`crate::projection_contract::PROJECTION_CONTRACT`] entry.
#[must_use]
pub fn projection_tier(key: &str) -> ProjectionTier {
    crate::projection_contract::contract_for(key).tier
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swift_projections_registry::SNAPSHOT_PROJECTIONS;

    /// Every registry `key` must be classified by [`projection_tier`] (i.e.
    /// have a contract entry). A new entry without a contract row trips the
    /// fail-closed panic here at commit time.
    #[test]
    fn every_registry_key_is_classified() {
        for entry in SNAPSHOT_PROJECTIONS {
            let _ = projection_tier(entry.key);
        }
    }

    /// A Tier-2 decodable built-in resolves to `KernelBuiltin`; a Tier-1 host
    /// registration resolves to `HostRegistered`. Spot-check the two boundaries.
    #[test]
    fn tier_lookup_matches_contract() {
        assert_eq!(
            projection_tier("relay_diagnostics"),
            ProjectionTier::KernelBuiltin
        );
        assert_eq!(projection_tier("wallet"), ProjectionTier::HostRegistered);
    }
}
