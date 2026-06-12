//! PR-4 composition seams for `KernelReducer`.
//!
//! These four methods let a wasm32 composition root wire the OP-feed engine
//! into the kernel without depending on `NmpApp` (which lives in `nmp-ffi`,
//! not available on wasm32) or the native actor thread:
//!
//! * `register_event_observer` — wire a `KernelEventObserver` into the fan-out slot.
//! * `register_typed_snapshot_projection` — wire a typed FlatBuffers projection.
//! * `active_account_handle` — read the active-account pubkey slot.
//! * `event_store_handle` — read the kernel event-store `Arc`.
//!
//! All methods delegate either to `self.kernel` (for slot handles that are
//! already `pub` there) or to `self.observer_slot` / `self.snapshot_slot`
//! (the per-reducer slots initialised in `KernelReducer::new`).
//!
//! # Doctrine
//!
//! * **D0** — surface types are all substrate-level: `Arc<dyn EventStore>`,
//!   `ActiveAccountSlot`, `KernelEventObserver`, `TypedProjectionData`.
//!   No NIP or app nouns.
//! * **D6** — poisoned mutex on register/lookup is a silent no-op; the
//!   caller never panics.
//! * **D8** — all methods are O(n-observers) at worst; no I/O, no blocking.

use std::sync::Arc;

use crate::actor::register_rust_observer;
use crate::slots::ActiveAccountSlot;
use crate::store::EventStore;
use crate::{KernelEventObserver, KernelEventObserverId, TypedProjectionData};

impl super::KernelReducer {
    // ── Event-observer slot seam ──────────────────────────────────────────

    /// Register an in-process Rust observer that will be called for every
    /// event the kernel accepts (i.e. returns `Inserted` or `Replaced` from
    /// `EventStore::insert`).
    ///
    /// Returns an opaque [`KernelEventObserverId`] the caller retains to
    /// unregister later. Registration is idempotent: the same `Arc` can be
    /// registered multiple times and fires once per registration.
    ///
    /// This is the wasm32 equivalent of `NmpApp::register_event_observer`.
    pub fn register_event_observer(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        register_rust_observer(&self.observer_slot, observer)
    }

    // ── Typed snapshot-projection seam ───────────────────────────────────

    /// Register a typed FlatBuffers snapshot projection under `key`.
    ///
    /// The closure `f` is called once per `make_update_frame` tick (on the
    /// wasm32 path that is the 1 Hz timer + explicit snapshot pulls). It
    /// returns `Some(TypedProjectionData)` when there is data to emit, or
    /// `None` to suppress the key for that tick.
    ///
    /// This is the wasm32 equivalent of
    /// `NmpApp::register_typed_snapshot_projection`.
    pub fn register_typed_snapshot_projection(
        &self,
        key: impl Into<String>,
        f: impl Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    ) {
        if let Ok(mut guard) = self.snapshot_slot.lock() {
            guard.register_typed(key, f);
        }
        // Poisoned mutex: D6 silent fail. The projection simply never
        // appears in snapshots — same graceful-degrade as a missing
        // registration.
    }

    // ── Kernel handle pass-throughs ───────────────────────────────────────

    /// Return the kernel's active-account pubkey slot.
    ///
    /// The returned [`ActiveAccountSlot`] is `Arc<Mutex<Option<String>>>`.
    /// Composition roots pass it to `ActiveFollowSet::new` (rung 4) so the
    /// follow-set producer can seed itself and respond to account switches
    /// without holding a reference to the full reducer.
    #[must_use]
    pub fn active_account_handle(&self) -> ActiveAccountSlot {
        self.kernel.active_account_handle()
    }

    /// Return the kernel's event-store handle.
    ///
    /// Used by the composition root to build an `EventLookup` closure
    /// (`Arc<dyn Fn(&str) -> Option<KernelEvent> + Send + Sync>`) for
    /// `register_op_feed`.  The returned `Arc<dyn EventStore>` is `Send +
    /// Sync`, so the closure can be stored across ticks without holding a
    /// `KernelReducer` borrow.
    #[must_use]
    pub fn event_store_handle(&self) -> Arc<dyn EventStore> {
        self.kernel.event_store_handle()
    }

}
