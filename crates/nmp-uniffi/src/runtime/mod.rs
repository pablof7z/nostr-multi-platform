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
//! ## Lifecycle callback (`nmp_app_set_lifecycle_callback`) — M14-D blocker
//!
//! The C-ABI `nmp_app_set_lifecycle_callback` is NOT migrated here. The
//! runtime's `lifecycle_observer` slot is an `Arc<Mutex<Option<...>>>` with a
//! snapshot-then-call pattern: the actor takes a snapshot UNDER the lock,
//! releases the mutex, then invokes the callback. This means
//! `set_lifecycle_observer(None)` can return while the actor is already past
//! the snapshot and about to call the old closure — no `in_flight` counter or
//! `Condvar` drain gate exists on this slot (unlike `UpdateListenerGate` and
//! `CapabilityCallbackGate`). Wrapping a `Box<dyn LifecycleSink>` in this slot
//! without a drain gate is use-after-free: the UniFFI ARC could be dropped
//! while the actor calls into it.
//!
//! This is the same structural gap as `ActionRegistry` (C4 → issue #2429).
//! A drain gate must be added to `LifecycleObserverSlot` before this callback
//! interface can be safely exposed via UniFFI.
//!
//! **Tracked as M14-D blocker**: add a `Condvar` + `in_flight` quiescence gate
//! to `LifecycleObserverSlot` in `nmp-core`, then expose `set_lifecycle_sink`
//! here in a follow-up PR.

pub mod config;
pub mod diag;
pub mod lifecycle;
