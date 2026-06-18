//! V-95 / issue #619 — `NmpAppBuilder::with_wallet` wires the NIP-47 wallet
//! stack as a typed *config-phase* step, before `start()` consumes the builder.
//!
//! # What this captures
//!
//! ADR-0052 rung 5.2: the three wallet `ActionModule`s are registered BY VALUE,
//! each owning a clone of the per-app `WalletRuntimeHandle` `register_wallet`
//! creates — there is no process-global `active_wallet_runtime`. The
//! install-before-dispatch ordering is type-enforced: `with_wallet()` returns
//! `NmpAppBuilder<Unstarted>`, and `start()` consumes the builder by move, so a
//! Rust caller cannot reach `start()` without having wired the wallet modules.
//!
//! These tests prove the builder step exists, wires the wallet stack during the
//! config phase, and still chains to `start()`. (Per-instance independence —
//! two builders, two wallets, no crosstalk — is proven end-to-end by
//! `nmp-testing/tests/k2_two_instance_wallet_isolation.rs`.)

use nmp_defaults::{NmpAppBuilder, RunConfig};
use nmp_ffi::{nmp_app_free, nmp_app_read_projection_json, nmp_app_stop, nmp_free_string};
use std::ffi::{CStr, CString};

/// `.with_wallet()` wires the wallet stack during config and type-state-advances
/// to `start()`. After start the `"wallet"` snapshot projection is registered
/// (it reads the per-app status slot the step installed), proving the wiring
/// landed on this instance.
#[test]
fn with_wallet_wires_stack_before_start() {
    // with_wallet() returns NmpAppBuilder<Unstarted>, so .in_memory() →
    // projection decision → .start() compiles and runs — the
    // install-before-dispatch ordering is type-enforced.
    let app = NmpAppBuilder::new()
        .with_wallet()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .start(RunConfig::default());
    assert!(!app.is_null(), "start() after with_wallet() returned null");

    // The `"wallet"` projection is registered by the step (it returns `null`
    // until a wallet connects, but the key resolves — an un-wired app returns a
    // null pointer for an unregistered projection key).
    let key = CString::new("wallet").unwrap();
    let ptr = nmp_app_read_projection_json(app, key.as_ptr());
    assert!(
        !ptr.is_null(),
        "with_wallet() must register the \"wallet\" snapshot projection"
    );
    // The value is JSON `null` (no wallet connected yet) — confirm it parses.
    let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_free_string(ptr);
    assert!(
        serde_json::from_str::<serde_json::Value>(&json).is_ok(),
        "the wallet projection must be valid JSON, got: {json}"
    );

    nmp_app_stop(app);
    nmp_app_free(app);
}

/// `.with_wallet()` composes with `register_defaults` + a storage choice in the
/// canonical composition-root order, and the whole pipeline starts.
#[test]
fn with_wallet_composes_with_register_defaults_and_storage() {
    let app = {
        let mut builder = NmpAppBuilder::new();
        nmp_defaults::register_defaults(&mut builder);
        builder
            .with_wallet()
            .storage_path("/tmp/nmp_test_v95_wallet")
            .consume_all_builtin_projections()
            .without_initial_relays()
            .start(RunConfig::default())
    };
    assert!(!app.is_null());
    // Per-instance (ADR-0052 rung 5.2): the wallet projection resolves on THIS
    // app regardless of which test ran first — no shared global.
    let key = CString::new("wallet").unwrap();
    let ptr = nmp_app_read_projection_json(app, key.as_ptr());
    assert!(!ptr.is_null(), "wallet projection must be registered");
    nmp_free_string(ptr);
    nmp_app_stop(app);
    nmp_app_free(app);
}
