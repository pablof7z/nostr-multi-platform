//! `RemoteSignerHandle` — re-exported from `nmp-signer-iface`.
//!
//! The actor-facing trait for signers whose key material lives outside the
//! kernel (NIP-46 today; NIP-55/hardware-wallets future) is dependency-light
//! interface vocabulary — it names only `nmp-signer-iface` types (`SignerOp`,
//! `SignedEvent`, `UnsignedEvent`). Issue #1720 moved its definition into that
//! tier-0 crate so `nmp-signers` (and other signer-facing crates) can name it
//! without depending on `nmp-core`. The kernel actor still only ever holds
//! `Box<dyn RemoteSignerHandle>` — D0 intact (`nmp-core` does not import
//! `nmp-signers`). Re-exported here so `nmp_core::RemoteSignerHandle` and the
//! actor-tree `crate::remote_signer::RemoteSignerHandle` import paths keep
//! resolving. This re-export is a staged migration aid, not a durable seam —
//! deletion gate: issue #1772 migrates every importer onto direct
//! `nmp_signer_iface::RemoteSignerHandle` imports and removes it.

pub use nmp_signer_iface::RemoteSignerHandle;
