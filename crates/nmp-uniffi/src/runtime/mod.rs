//! Runtime lifecycle, storage config, projection config, and diagnostics
//! UniFFI surface.
//!
//! `nmp-uniffi` is the sole native binding surface for runtime lifecycle,
//! storage/projection configuration, and diagnostics (M14 complete; the
//! legacy `nmp-ffi` C-ABI crate has been deleted). Each sub-module adds a
//! `#[uniffi::export] impl NmpApp` block exposing typed methods.
//!
//! ## Module layout
//!
//! | Module     | UniFFI methods                                                             |
//! |------------|-----------------------------------------------------------------------------|
//! | `lifecycle`| `lifecycle_foreground`, `lifecycle_background`, `is_alive`                 |
//! | `config`   | `set_storage_path`, `declare_incremental_apply`, `declare_consumed_projections`, `consume_all_builtin_projections` |
//! | `diag`     | `intent_dispatch`, `debug_info`                                            |
//!
//! The lifecycle callback uses the `LifecycleObserverGate` drain contract
//! landed by #2439.

pub mod config;
pub mod diag;
pub mod lifecycle;

/// Rust→shell lifecycle observer.
#[uniffi::export(callback_interface)]
pub trait LifecycleSink: Send + Sync {
    fn on_lifecycle_phase(&self, phase: u32);
}
