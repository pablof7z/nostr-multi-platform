//! Signing value types — re-exported from `nmp-signer-iface`.
//!
//! `UnsignedEvent`, `SignedEvent`, and `SigningError` are dependency-light
//! NIP-01 event vocabulary (serde value types, no kernel behavior). Issue #1720
//! moved their definitions into the tier-0 `nmp-signer-iface` crate so signer
//! and protocol crates can name them without depending on `nmp-core`. They are
//! re-exported here so the kernel-side and protocol-crate
//! `nmp_core::substrate::{SignedEvent, UnsignedEvent, SigningError}` import
//! paths keep resolving. This re-export is a staged migration aid, not a
//! durable seam — deletion gate: issue #1772 migrates every importer onto
//! direct `nmp_signer_iface` imports and removes it.

pub use nmp_signer_iface::{SignedEvent, SigningError, UnsignedEvent};
