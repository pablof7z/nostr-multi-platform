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

use nmp_native_runtime::{NmpAppBuilder, RunConfig};
mod common;
use common::*;

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

    // The generic JSON lane is deleted (rule A6). Check the typed registry.
    let app_ref: &NmpApp = unsafe { &*app };
    let typed_keys = app_ref.registered_typed_projection_keys();
    assert!(
        typed_keys.contains(&"wallet".to_string()),
        "with_wallet() must register the \"wallet\" typed snapshot projection"
    );

    stop_app(app);
    free_app_ptr(app);
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
    // The generic JSON lane is deleted (rule A6). Check the typed registry.
    let app_ref: &NmpApp = unsafe { &*app };
    let typed_keys = app_ref.registered_typed_projection_keys();
    assert!(
        typed_keys.contains(&"wallet".to_string()),
        "wallet projection must be registered"
    );
    stop_app(app);
    free_app_ptr(app);
}
