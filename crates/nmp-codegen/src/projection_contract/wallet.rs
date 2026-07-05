//! Wallet host-registered projection contract entries.
//!
//! Extracted from `table.rs` to keep each file under the 500-LOC hard cap.

use super::{DeclarationPolicy, PresencePolicy, ProjectionContract, ProjectionTier};

pub const WALLET_STATUS: ProjectionContract = ProjectionContract {
    key: "wallet",
    tier: ProjectionTier::HostRegistered,
    producer: "nmp-wallet projection contract; nmp-nip47 compatibility implementation",
    owner_claim: "projection.wallet",
    schema_id: "nmp.nip47.wallet",
    file_identifier: "NWST",
    // nmp-nip47 wire/typed_fb::SCHEMA_VERSION
    version: 1,
    declaration_policy: DeclarationPolicy::RegistrationGated,
    dependency_versions: &[],
    presence_policy: PresencePolicy::None,
};

// The MERGED multi-backend wallet projection (#2915). Registered by
// `nmp_wallet::register` under a DISTINCT key from the `"wallet"` NWST
// sidecar above (which nmp-nip47 still owns) so the two coexist. No iOS
// Swift consumer yet — see NOT_SWIFT_PRESENTED in the contract tests.
//
// v3 (#2880, epic #2864): gained `discovered_mints`, the NIP-87
// web-of-trust-scoped, capability-fail-closed discovered-mints view folded in
// from the sibling `MintDiscoveryRuntime` at
// `register::wallet_merged_typed_projection`.
//
// v4 (#2880 unwind): `discovered_mints` REMOVED — NIP-87 mint discovery moved
// to the standalone `nmp-mint-discovery` crate's own `"mint_discovery"`
// projection (see `MINT_DISCOVERY` below). The wire slot is deprecated in
// place, not reused (`crates/nmp-wallet/schema/wallet_projection.fbs`).
pub const WALLET_MERGED: ProjectionContract = ProjectionContract {
    key: "wallet.merged",
    tier: ProjectionTier::HostRegistered,
    producer: "nmp-wallet register (merged WalletRuntime::snapshot sidecar)",
    owner_claim: "projection.wallet.merged",
    schema_id: "nmp.wallet.merged",
    file_identifier: "NWMP",
    // nmp-wallet projection_wire::SCHEMA_VERSION
    version: 4,
    declaration_policy: DeclarationPolicy::RegistrationGated,
    dependency_versions: &[],
    presence_policy: PresencePolicy::None,
};
