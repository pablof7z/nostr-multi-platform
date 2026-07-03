//! Cashu mint HTTP client and cryptographic primitives.
//!
//! # Modules
//!
//! - [`crypto`] — DHKE blind signatures and DLEQ proof verification (NUT-00, NUT-12).
//! - [`types`] — HTTP API request/response types (NUT-01 through NUT-07).
//! - `client` — Synchronous HTTP client wrapping the above. Requires the
//!   `native` feature (uses `ureq`); the codec/type surface above stays
//!   HTTP-free and always-compiled.

#[cfg(feature = "native")]
pub mod client;
pub mod crypto;
pub mod types;

#[cfg(feature = "native")]
pub use client::{split_amount, MintClient};
pub use crypto::{blind_message, hash_to_curve, random_secret, unblind_signature, verify_dleq, DleqProof};
pub use types::Proof;
