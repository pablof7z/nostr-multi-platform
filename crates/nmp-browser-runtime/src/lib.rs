//! Browser runtime for WASM.
//!
//! This crate provides a typed capability envelope and provider registry for browser
//! signer backends (NIP-07, nsec/local-key, NIP-46). Each signer backend is a provider,
//! not a new runtime branch. The capability contract is defined here; consumers depend
//! on this trait to integrate signers.
//!
//! ## Security Contract
//!
//! **Secret redaction (D13):** Capability requests where `secret_bearing == true`
//! (e.g., nsec input) MUST NEVER enter debug logs, snapshots, action tags, or
//! dispatch history. Only the redacted account prefix (if any) is permitted in
//! diagnostics. The caller (kernel) is responsible for redacting all summaries
//! when iterating requests — see the Wave-3 projection track (#2075).

pub mod capability;

pub use capability::{
    CapabilityDispatch, CapabilityFailureKind, CapabilityId, CapabilityMeta, CapabilityOutcome,
    CapabilityProvider, CapabilityRegistry, CapabilityRequest,
};
