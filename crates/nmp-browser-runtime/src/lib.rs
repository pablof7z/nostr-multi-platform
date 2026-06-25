//! Browser platform adapter for NMP (ADR-0065).
//!
//! # Owns
//! - the Worker event-loop runtime driving a single `KernelReducer` (D4);
//! - the browser WebSocket transport adapter (transport-only);
//! - the capability/signer provider registry;
//! - browser timer/clock seams;
//! - the `BrowserAppBuilder` typed composition root.
//!
//! # Does NOT own
//! - routing/outbox policy (that is `nmp-router`/kernel);
//! - signing policy or signer-provider choice semantics (that is `nmp-signers`/`nmp-signer-broker`);
//! - NIP modules, protocol defaults, app defaults, projection policy, persistence policy;
//! - the wasm-bindgen ABI surface (that is the sibling `nmp-wasm` ABI shell).
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

#[cfg(target_arch = "wasm32")]
pub fn install_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Placeholder marker so the crate compiles before the builder/runtime land in
/// later tracks (#2046/#2057/#2058). Replaced by `BrowserAppBuilder` in Wave 3.
pub struct BrowserRuntimePlaceholder;
