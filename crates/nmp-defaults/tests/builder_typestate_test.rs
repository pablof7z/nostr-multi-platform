//! Tests for `NmpAppBuilder` — V-94 typestate enforcement.
//!
//! # What this file covers
//!
//! 1. **Happy path** — builder constructed, defaults wired, storage chosen,
//!    projection decision made, `start()` called, resulting pointer non-null.
//! 2. **Storage explicit opt-in** — both `.storage_path(p)` and `.in_memory()`
//!    transition to `StorageSet`.
//! 3. **`register_defaults` through the builder** — the builder implements
//!    `AppHost + ActionRegistrar` so the existing free function still works.
//! 4. **Drop guard** — a builder dropped without `start()` does not leak
//!    (this is structural; the test merely checks the builder allocates and
//!    frees without aborting/leaking in sanitiser runs).
//! 5. **Compile-fail proof** — two `compile_fail` doctests in `builder.rs`
//!    document that calling `.start()` before a storage choice OR before a
//!    projection-consumption decision is a compile error (ADR-0053 DEBT 2).
//! 6. **ADR-0053 DEBT 2 narrowing** — `.declare_consumed_projections(keys)`
//!    advances the typestate AND narrows; `.consume_all_builtin_projections()`
//!    advances the typestate without narrowing (explicit firehose opt-in).
//!
//! # What this file does NOT try to test
//!
//! `start()` actually spinning up the actor thread and exercising the kernel
//! is integration-test territory (the `nmp-testing` crate). These tests
//! focus on the wiring-phase guarantees.

mod common;
use common::*;
use nmp_native_runtime::{NmpAppBuilder, RunConfig};

// ── helper ───────────────────────────────────────────────────────────────────

/// Start a builder through the full happy path and return the started pointer.
/// The caller is responsible for `free_app_ptr(ptr)`.
fn start_default() -> *mut NmpApp {
    let app = NmpAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .start(RunConfig::default());
    assert!(!app.is_null(), "start() returned null pointer");
    app
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn builder_new_and_in_memory_start_returns_non_null() {
    // Minimal happy path: no extra wiring, explicit in-memory opt-in.
    let app = start_default();
    stop_app(app);
    free_app_ptr(app);
}

#[test]
fn builder_storage_path_start_returns_non_null() {
    // Use `.storage_path` branch (even with an empty string, which the
    // C-ABI setter treats as "unset" → in-memory fallback).
    // The important invariant is that the type-state transition compiles
    // and the pointer is non-null.
    let app = NmpAppBuilder::new()
        .storage_path("/tmp/nmp_test_v94")
        .consume_all_builtin_projections()
        .without_initial_relays()
        .start(RunConfig::default());
    assert!(!app.is_null(), "start() returned null after storage_path()");
    stop_app(app);
    free_app_ptr(app);
}

#[test]
fn builder_implements_apphost_for_register_defaults() {
    // The builder implements `AppHost + ActionRegistrar`, so the existing
    // free function `register_defaults(&mut impl AppHost)` works against it.
    // This is the primary consumer model: the composition root calls
    // `register_defaults` on the builder before calling `start()`.
    let app = {
        let mut builder = NmpAppBuilder::new();
        nmp_defaults::register_defaults(&mut builder);
        builder
            .in_memory()
            .consume_all_builtin_projections()
            .without_initial_relays()
            .start(RunConfig::default())
    };
    assert!(!app.is_null());
    stop_app(app);
    free_app_ptr(app);
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
        nmp_defaults::register_defaults(&mut builder);
        builder
            .in_memory()
            .consume_all_builtin_projections()
            .without_initial_relays()
            .start(cfg)
    };
    assert!(!app.is_null());
    stop_app(app);
    free_app_ptr(app);
}

/// ADR-0053 DEBT 2 — the narrowing path. `.declare_consumed_projections(keys)`
/// is the typestate-advancing consuming method (`StorageSet →
/// ProjectionsDeclared`): it declares the set into the `SnapshotRegistry` AND
/// unlocks `.start()`. The declared set survives through `start()` and the
/// `consumed_projections_are_narrowing()` query returns `true`.
#[test]
fn builder_declare_consumed_projections_narrows_and_advances_typestate() {
    let app = NmpAppBuilder::new()
        .in_memory()
        .declare_consumed_projections(["profile", "accounts"])
        .without_initial_relays()
        .start(RunConfig::default());
    assert!(!app.is_null());
    // SAFETY: `nmp_app_start` returned non-null; the pointer is valid for the
    // duration of this test (we call `nmp_app_stop` + `nmp_app_free` below).
    assert!(
        unsafe { &*app }.consumed_projections_are_narrowing(),
        "after declaring a non-empty set the app must be in narrowing state"
    );
    stop_app(app);
    free_app_ptr(app);
}

/// ADR-0053 DEBT 2 — the explicit firehose opt-out.
/// `.consume_all_builtin_projections()` advances the typestate (unlocking
/// `.start()`) WITHOUT narrowing: the kernel's empty=permissive semantic
/// (Decision 4) is preserved, so `consumed_projections_are_narrowing()` is
/// `false`. The point is that "everything" is now an explicit, greppable call
/// rather than a silent default.
#[test]
fn builder_consume_all_builtin_projections_is_not_narrowing() {
    let app = NmpAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .start(RunConfig::default());
    assert!(!app.is_null());
    // SAFETY: non-null pointer from start(); freed below.
    assert!(
        !unsafe { &*app }.consumed_projections_are_narrowing(),
        "consume_all_builtin_projections() must leave the app permissive (not narrowing)"
    );
    stop_app(app);
    free_app_ptr(app);
}

/// #1493 — `.with_relays(...)` is the typestate-advancing relay decision
/// (`ProjectionsDeclared → RelaysDeclared`): it declares the app's initial
/// relay set AND unlocks `.start()`. NMP supplies no relay default, so this is
/// the only way (besides `.without_initial_relays()`) to reach `start()`.
#[test]
fn builder_with_relays_advances_typestate_and_starts() {
    let app = NmpAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .with_relays([("wss://app-owned.relay/", "both")])
        .start(RunConfig::default());
    assert!(!app.is_null(), "with_relays → start must return non-null");
    stop_app(app);
    free_app_ptr(app);
}

/// #1493 — `.with_relays(empty)` panics: a no-relay start must be the explicit
/// `.without_initial_relays()` choice, never a silent empty declaration.
#[test]
#[should_panic(expected = "without_initial_relays")]
fn builder_with_relays_empty_panics() {
    let _app = NmpAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .with_relays(Vec::<(String, String)>::new())
        .start(RunConfig::default());
}

/// ADR-0053 DEBT 2 — declarations are additive across the trait method and the
/// typestate method. A protocol crate may add keys via `AppHost`
/// (`&self`-receiver) during `register`, then the composition root finalizes
/// the decision via the consuming `.declare_consumed_projections` typestate
/// method. Both unions land; the result is narrowing.
#[test]
fn builder_declared_projections_union_across_trait_and_typestate_methods() {
    use nmp_core::substrate::SnapshotProjectionRegistrar;
    let builder = NmpAppBuilder::new();
    // Simulate a protocol-crate additive declaration via the narrow
    // SnapshotProjectionRegistrar trait (&self receiver — does NOT advance the
    // typestate).
    SnapshotProjectionRegistrar::declare_consumed_projections(&builder, ["profile"]);
    // Composition root finalizes via the consuming typestate method.
    let app = builder
        .in_memory()
        .declare_consumed_projections(["accounts"])
        .without_initial_relays()
        .start(RunConfig::default());
    assert!(!app.is_null());
    // SAFETY: non-null pointer from start(); freed below.
    assert!(
        unsafe { &*app }.consumed_projections_are_narrowing(),
        "the union of both declarations must be narrowing"
    );
    stop_app(app);
    free_app_ptr(app);
}
