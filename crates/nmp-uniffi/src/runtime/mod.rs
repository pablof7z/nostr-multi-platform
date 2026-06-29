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
//! ## Lifecycle callback (`set_lifecycle_sink`) — M14-C-tail (#2429)
//!
//! `nmp_app_set_lifecycle_callback` is now mirrored by the UniFFI
//! [`LifecycleSink`] callback interface + [`NmpApp::set_lifecycle_sink`]
//! (`runtime/lifecycle.rs`). The `nmp-core` `LifecycleObserverGate` gained an
//! `in_flight` + `Condvar` drain (the same gate that protects the capability
//! socket and the update listener), so a `Box<dyn LifecycleSink>` ARC can be
//! registered and released safely: after `set_lifecycle_sink(None)` (or a
//! replace) returns, the previous sink is neither registered nor mid-invocation.
//!
//! Both registration paths (the C-ABI `LifecycleObserverFn` and this UniFFI
//! sink) share that one gate, last-writer-wins. The C-ABI symbol stays additive
//! until M14-D.
//!
//! ## `LifecycleSink` (push observer)
//!
//! The actor folds a scenePhase change into the kernel and, on a meaningful
//! transition, calls `on_lifecycle_transition(phase)` with the phase wire
//! discriminant (`0` = foreground, `1` = background — the same codes the C-ABI
//! `LifecycleObserverFn` receives). The phase code is a copied `u32`; no Rust
//! lock is held across the foreign call. Implementations MUST NOT call
//! `set_lifecycle_sink` from inside the callback (reentrancy deadlocks the
//! quiescence gate).

pub mod config;
pub mod diag;
pub mod lifecycle;

/// Phase wire discriminant for [`LifecycleSink::on_lifecycle_transition`]:
/// the app entered the foreground (`scenePhase == .active` on iOS).
pub const LIFECYCLE_PHASE_FOREGROUND: u32 = 0;
/// Phase wire discriminant for [`LifecycleSink::on_lifecycle_transition`]:
/// the app entered the background.
pub const LIFECYCLE_PHASE_BACKGROUND: u32 = 1;

/// Rust→shell push observer fired on a meaningful app-lifecycle transition.
///
/// # Contract
///
/// * `phase` is the wire discriminant ([`LIFECYCLE_PHASE_FOREGROUND`] /
///   [`LIFECYCLE_PHASE_BACKGROUND`]) — a copied `u32`, no Rust lock held across
///   the call.
/// * The observer fires only on meaningful transitions (debounced by the
///   kernel; rapid `Foreground→Foreground` is a no-op).
/// * Implementations MUST NOT call `set_lifecycle_sink` from inside this method:
///   the setter waits for the in-flight callback to drain, so re-entry would
///   deadlock the quiescence gate.
#[uniffi::export(callback_interface)]
pub trait LifecycleSink: Send + Sync {
    fn on_lifecycle_transition(&self, phase: u32);
}
