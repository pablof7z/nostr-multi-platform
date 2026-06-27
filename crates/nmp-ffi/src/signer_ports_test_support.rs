//! Test-support seam for the K2 rung 5.3 per-app signer-port oracle
//! (`nmp-testing/tests/k2_per_app_signer_ports_isolation.rs`).
//!
//! ADR-0052 §D3. These `pub fn`s (Rust ABI, not C-ABI) let an integration test
//! install a recording hook into an app's per-app bunker / NIP-55 hook slot and
//! invoke it through the SAME slot the actor's `IdentityRuntime` reads —
//! proving the slot is instance-scoped (two apps don't crosstalk) and survives
//! `nmp_app_free` + recreate (the Android process-reuse dead-end the old
//! `OnceLock` globals could not satisfy), without standing up the NIP-46 wire.
//!
//! The whole module is gated on `cfg(any(test, feature = "test-support"))`; it
//! is never part of the production FFI ABI (D0).
#![cfg(any(test, feature = "test-support"))]

use super::NmpApp;
use nmp_core::{BunkerHookFn, ExternalSignerHookFn};

/// Install a bunker hook into `app`'s per-app slot (the same slot
/// `nmp_signer_broker_init` installs the real broker hook into). Null-safe.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn install_bunker_hook_for_test(app: *mut NmpApp, hook: BunkerHookFn) {
    nmp_native_runtime::install_bunker_hook_for_test(app, hook);
}

/// Invoke `app`'s installed bunker connect hook through its per-app slot.
/// Returns `true` iff a hook is installed (and fired). Null-safe (`false`).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn invoke_bunker_connect_hook_for_test(app: *mut NmpApp, uri: &str) -> bool {
    nmp_native_runtime::invoke_bunker_connect_hook_for_test(app, uri)
}

/// Install a NIP-55 external-signer restore hook into `app`'s per-app slot.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn install_external_signer_hook_for_test(app: *mut NmpApp, hook: ExternalSignerHookFn) {
    nmp_native_runtime::install_external_signer_hook_for_test(app, hook);
}

/// Invoke `app`'s installed NIP-55 restore hook through its per-app slot.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn invoke_external_signer_restore_hook_for_test(app: *mut NmpApp, payload_json: &str) -> bool {
    nmp_native_runtime::invoke_external_signer_restore_hook_for_test(app, payload_json)
}
