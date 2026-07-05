//! `nmp-mint-discovery`'s own host-registered projection contract entry.
//!
//! Extracted into its own file (mirroring `wallet.rs`) to keep each
//! projection-contract file under the 500-LOC hard cap.

use super::{DeclarationPolicy, PresencePolicy, ProjectionContract, ProjectionTier};

// The WoT-scoped, capability-fail-closed NIP-87 discovered-mints view
// (#2880, epic #2864). Registered by `nmp_mint_discovery::register` under a
// fresh key distinct from any host/wallet-owned sidecar — any app that
// composes `nmp-mint-discovery` gets this projection, whether or not it also
// composes `nmp-wallet` (which no longer folds discovery into its own
// `"wallet.merged"` projection; see that crate's `WALLET_MERGED` v4 note).
pub const MINT_DISCOVERY: ProjectionContract = ProjectionContract {
    key: "mint_discovery",
    tier: ProjectionTier::HostRegistered,
    producer: "nmp-mint-discovery register (MintDiscoveryRuntime::snapshot sidecar)",
    owner_claim: "projection.mint_discovery",
    schema_id: "nmp.mint_discovery",
    file_identifier: "NMDS",
    // nmp-mint-discovery projection_wire::SCHEMA_VERSION
    version: 1,
    declaration_policy: DeclarationPolicy::RegistrationGated,
    dependency_versions: &[],
    presence_policy: PresencePolicy::None,
};
