//! Tests for `NmpAppBuilder` — V-94 typestate enforcement.
//!
//! # What this file covers
//!
//! 1. **Happy path** — builder constructed, defaults wired, storage chosen,
//!    `start()` called, resulting pointer non-null.
//! 2. **Storage explicit opt-in** — both `.storage_path(p)` and `.in_memory()`
//!    transition to `StorageSet` and allow `start()`.
//! 3. **`register_defaults` through the builder** — the builder implements
//!    `AppHost + ActionRegistrar` so the existing free function still works.
//! 4. **Drop guard** — a builder dropped without `start()` does not leak
//!    (this is structural; the test merely checks the builder allocates and
//!    frees without aborting/leaking in sanitiser runs).
//! 5. **Compile-fail proof** — a `compile_fail` doctest in `builder.rs`
//!    documents that calling `.start()` before a storage choice is a compile
//!    error.
//!
//! # What this file does NOT try to test
//!
//! `start()` actually spinning up the actor thread and exercising the kernel
//! is integration-test territory (the `nmp-testing` crate). These tests
//! focus on the wiring-phase guarantees.

use nmp_ffi::{nmp_app_free, nmp_app_stop};
use nmp_app_template::{NmpAppBuilder, RunConfig};

// ── helper ───────────────────────────────────────────────────────────────────

/// Start a builder through the full happy path and return the started pointer.
/// The caller is responsible for `nmp_app_free(ptr)`.
fn start_default() -> *mut nmp_ffi::NmpApp {
    let app = NmpAppBuilder::new()
        .in_memory()
        .start(RunConfig::default());
    assert!(!app.is_null(), "start() returned null pointer");
    app
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn builder_new_and_in_memory_start_returns_non_null() {
    // Minimal happy path: no extra wiring, explicit in-memory opt-in.
    let app = start_default();
    nmp_app_stop(app);
    nmp_app_free(app);
}

#[test]
fn builder_storage_path_start_returns_non_null() {
    // Use `.storage_path` branch (even with an empty string, which the
    // C-ABI setter treats as "unset" → in-memory fallback).
    // The important invariant is that the type-state transition compiles
    // and the pointer is non-null.
    let app = NmpAppBuilder::new()
        .storage_path("/tmp/nmp_test_v94")
        .start(RunConfig::default());
    assert!(!app.is_null(), "start() returned null after storage_path()");
    nmp_app_stop(app);
    nmp_app_free(app);
}

#[test]
fn builder_implements_apphost_for_register_defaults() {
    // The builder implements `AppHost + ActionRegistrar`, so the existing
    // free function `register_defaults(&mut impl AppHost)` works against it.
    // This is the primary consumer model: the composition root calls
    // `register_defaults` on the builder before calling `start()`.
    let app = {
        let mut builder = NmpAppBuilder::new();
        nmp_app_template::register_defaults(&mut builder);
        builder.in_memory().start(RunConfig::default())
    };
    assert!(!app.is_null());
    nmp_app_stop(app);
    nmp_app_free(app);
}

#[test]
fn builder_drop_without_start_does_not_panic_or_leak() {
    // A builder that is constructed but never started should free the inner
    // `NmpApp` on drop — no memory leak, no double-free, no panic.
    //
    // Under AddressSanitizer or Valgrind this test catches actual leaks;
    // in a plain `cargo test` run it verifies there is no panic/abort.
    {
        let _builder = NmpAppBuilder::new();
        // Drop here — `Drop::drop` calls `nmp_app_free`.
    }
    // If we reach here without abort the drop guard worked.
}

#[test]
fn builder_drop_after_in_memory_without_start_does_not_panic_or_leak() {
    // Same as above but the storage choice has been made (StorageSet state).
    {
        let _builder = NmpAppBuilder::new().in_memory();
    }
}

#[test]
fn run_config_default_is_sensible() {
    let cfg = RunConfig::default();
    assert!(cfg.visible_limit > 0, "visible_limit must be positive");
    assert!(cfg.emit_hz > 0, "emit_hz must be positive");
}

#[test]
fn builder_full_pipeline_with_register_defaults_and_custom_run_config() {
    // End-to-end: register_defaults → storage choice → custom RunConfig → start.
    let cfg = RunConfig {
        visible_limit: 50,
        emit_hz: 2,
    };
    let app = {
        let mut builder = NmpAppBuilder::new();
        nmp_app_template::register_defaults(&mut builder);
        builder.in_memory().start(cfg)
    };
    assert!(!app.is_null());
    nmp_app_stop(app);
    nmp_app_free(app);
}
