//! Runtime lifecycle, storage config, projection config, and diagnostics —
//! M14-C6 UniFFI surface.
//!
//! Migrates the C-ABI symbols from `nmp-ffi/src/{lifecycle,storage,snapshot,
//! debug_info,intent_ffi}` to typed `#[uniffi::export] impl NmpApp` methods.
//! This is **additive** — the C-ABI symbols are NOT deleted here
//! (transitional until M14-D).
//!
//! ## Module layout
//!
//! | Module     | UniFFI methods                                                             | C-ABI counterpart                   |
//! |------------|----------------------------------------------------------------------------|-------------------------------------|
//! | `lifecycle`| `lifecycle_foreground`, `lifecycle_background`, `is_alive`                 | `nmp-ffi/src/lifecycle.rs`          |
//! | `config`   | `set_storage_path`, `declare_incremental_apply`, `declare_consumed_projections`, `consume_all_builtin_projections` | `nmp-ffi/src/{storage,snapshot}.rs` |
//! | `diag`     | `intent_dispatch`, `debug_info`                                            | `nmp-ffi/src/{intent_ffi,debug_info}.rs` |
//!
//! The lifecycle callback is available here because #2439 landed the
//! `LifecycleObserverGate` drain contract on `master` before this restore.

pub mod config;
pub mod diag;
pub mod lifecycle;

/// Rust→shell lifecycle observer.
#[uniffi::export(callback_interface)]
pub trait LifecycleSink: Send + Sync {
    fn on_lifecycle_phase(&self, phase: u32);
}
