//! #2919 — `NmpAppBuilder::with_wallet()` used to drop `Handles::runtime`,
//! the only handle exposing `WalletRuntime::snapshot()` (the merged bounded
//! "wallet" projection). This proves a builder-path consumer can now
//! retrieve it via `.wallet_runtime()` and reach `.snapshot()`, both before
//! and after `.start()` consumes the builder.
#![cfg(feature = "wallet")]

use nmp_native_runtime::{NmpApp, NmpAppBuilder, RunConfig};

fn free_app(app: *mut NmpApp) {
    assert!(!app.is_null(), "builder returned a null app pointer");
    // SAFETY: `NmpAppBuilder::start` transfers ownership of the pointer to
    // the caller. This test is the owner and drops it exactly once.
    unsafe {
        (&*app).stop_runtime();
        drop(Box::from_raw(app));
    }
}

#[test]
fn with_wallet_exposes_a_retrievable_wallet_runtime() {
    let builder = NmpAppBuilder::new()
        .with_wallet()
        .expect("with_wallet registers cleanly on a fresh builder");

    let wallet_runtime = builder
        .wallet_runtime()
        .expect("with_wallet must stash a retrievable WalletRuntime handle (#2919)");

    // Reachability is the point of #2919 — the projection itself starts
    // empty on a freshly wired runtime.
    let snapshot = wallet_runtime.snapshot();
    assert!(snapshot.balances.is_empty());

    let app = builder
        .in_memory()
        .declare_consumed_projections(["profile"])
        .without_initial_relays()
        .start(RunConfig::default());

    // `Arc<WalletRuntime>` is independent of the builder/app lifetime — the
    // handle obtained before `start()` keeps working after the builder is
    // consumed.
    let _snapshot_after_start = wallet_runtime.snapshot();

    free_app(app);
}

#[test]
fn wallet_runtime_is_none_before_with_wallet() {
    let builder = NmpAppBuilder::new();
    assert!(
        builder.wallet_runtime().is_none(),
        "wallet_runtime() must be None until .with_wallet() has run"
    );
}
