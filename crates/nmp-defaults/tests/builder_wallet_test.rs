//! V-95 / issue #619 — `NmpAppBuilder::with_wallet` installs the NIP-47 wallet
//! runtime as a typed *config-phase* step, before `start()` consumes the
//! builder.
//!
//! # The bug this captures
//!
//! `WalletConnectModule` / `WalletDisconnectModule` / `WalletPayInvoiceModule`
//! `::execute` read the process-wide wallet runtime via
//! `nmp_nip47::active_wallet_runtime()` and fail if it was never installed. The
//! ordering "install before dispatch" used to live in an app crate with no
//! compile-time enforcement. Folding the install into a builder step means a
//! Rust caller cannot reach `start()` (which consumes the builder by move)
//! without the runtime installed — the ordering is now type-enforced.
//!
//! These tests prove the builder step exists, installs the runtime during the
//! config phase, and still chains to `start()`.

use nmp_defaults::{NmpAppBuilder, RunConfig};
use nmp_ffi::{nmp_app_free, nmp_app_stop};

/// `.with_wallet()` installs the process-wide wallet runtime *during config*,
/// before `start()`. After the call, `active_wallet_runtime()` is `Some` — the
/// precondition every `nmp.wallet.*` action `execute` checks.
#[test]
fn with_wallet_installs_runtime_before_start() {
    // Build through the config phase only — do NOT call start() yet.
    let builder = NmpAppBuilder::new().with_wallet();

    // The runtime must be installed at this point (config phase), so any wallet
    // action dispatched after start would find it. This is the install-before-
    // dispatch ordering, now reached as a typed builder step.
    assert!(
        nmp_nip47::active_wallet_runtime().is_some(),
        "with_wallet() must install the wallet runtime during the config phase, \
         before start() — active_wallet_runtime() should be Some"
    );

    // The builder still type-state-advances to start: with_wallet() returns
    // NmpAppBuilder<Unstarted>, so .in_memory().start() compiles and runs.
    let app = builder.in_memory().start(RunConfig::default());
    assert!(!app.is_null(), "start() after with_wallet() returned null");
    nmp_app_stop(app);
    nmp_app_free(app);
}

/// `.with_wallet()` composes with `register_defaults` and a storage choice in
/// the canonical composition-root order, and the whole pipeline starts.
#[test]
fn with_wallet_composes_with_register_defaults_and_storage() {
    let app = {
        let mut builder = NmpAppBuilder::new();
        nmp_defaults::register_defaults(&mut builder);
        builder
            .with_wallet()
            .storage_path("/tmp/nmp_test_v95_wallet")
            .start(RunConfig::default())
    };
    assert!(!app.is_null());
    // Runtime installed regardless of which test ran first (process-global
    // OnceLock; first install wins, subsequent are silent no-ops).
    assert!(nmp_nip47::active_wallet_runtime().is_some());
    nmp_app_stop(app);
    nmp_app_free(app);
}
