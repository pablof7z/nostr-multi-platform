//! Browser-facing protocol types for NMP (ADR-0065 / ADR-0067).
//!
//! # Purpose
//!
//! `nmp-wasm` is a protocol-type crate only — it does **not** own:
//! - Routing, signing policy, or signer-provider choice (Wave 3 provider registry)
//! - NIP modules, protocol defaults, or app defaults
//! - Projection policy, persistence policy, retry policy, or account state
//! - wasm-bindgen exports, Worker lifecycle, storage open, or JS callbacks
//!
//! The browser runtime (composition, lifecycle, wasm-bindgen exports, and
//! policy) is owned by `nmp-browser-runtime`. This crate now owns only the
//! serializable wire contract retained for older Rust consumers of the protocol
//! data types.

pub mod protocol;

pub use protocol::{
    BeginSign, CapabilityFailure, CapabilityResult, ClientHello, DegradedMode,
    DeliverSignerResponse, DispatchBytes, IdentityRelayPermission, RelayBootstrapEntry, ReleaseRef,
    ResolveRef, RuntimeStatus, SetIdentity, StartConfig, WorkerEvent, WorkerRequest,
};
