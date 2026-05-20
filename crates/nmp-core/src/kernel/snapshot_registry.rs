//! Host-extensible snapshot output — the `nmp_app_register_snapshot_projection`
//! seam.
//!
//! This is the output-side counterpart to the action-registry seam
//! (`nmp_app_register_action_module` / `nmp_app_register_action_executor`).
//! Where the action registry lets a host *dispatch* a custom namespace, the
//! snapshot registry lets a host *project* a custom namespace into the
//! snapshot every tick emits.
//!
//! ## The problem
//!
//! [`KernelSnapshot`](super::types::KernelSnapshot) is a sealed social wire
//! schema — `profile`, `items`, `author_view`, `thread_view`, … are baked
//! into the JSON every shell decodes. A non-social app (marketplace, todo
//! list, …) receives a snapshot it cannot make sense of.
//!
//! ## The seam
//!
//! A host registers a **snapshot projection**: a closure that runs on every
//! tick and produces a JSON value appended to the snapshot under a
//! host-chosen key. A marketplace registers `"market.listings"`, a todo app
//! registers `"todo.items"` — each gets its own namespace in
//! `KernelSnapshot::projections` without touching the typed social fields.
//!
//! ## Threading
//!
//! The registry is stored behind a shared [`SnapshotProjectionSlot`]
//! (`Arc<Mutex<…>>`), the same pattern as the kernel event observer slot:
//!
//! - the FFI / Rust registration path mutates the inner registry through one
//!   `Arc` clone (during host init);
//! - the actor thread carries another clone, binds it onto the kernel via
//!   [`Kernel::set_snapshot_projection_handle`], and the kernel reads it
//!   inside `make_update`.
//!
//! Because the box crosses thread boundaries it must be `Send + Sync`.
//!
//! ## D8 — non-blocking
//!
//! A projection closure runs on the actor thread **inside the snapshot
//! tick**. It MUST be cheap and non-blocking — no I/O, no mutex waits, no
//! relay round-trips. A blocking closure stalls every subsequent snapshot
//! and freezes the host's update stream.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::Kernel;

/// A host-registered projection closure.
///
/// Takes no arguments — a snapshot tick is a pull, the kernel drives it — and
/// returns the JSON value to append under the registered key. `Send + Sync`
/// because the box lives behind an `Arc<Mutex<…>>` shared with the actor
/// thread (D8: the closure itself must also be non-blocking).
pub type ProjectionFn = Box<dyn Fn() -> serde_json::Value + Send + Sync + 'static>;

/// Append-only registry of host-supplied snapshot projections.
///
/// Stored as a `Vec` of `(key, closure)` pairs rather than a `HashMap` so the
/// registration order is preserved and a host registering the same key twice
/// is a deterministic last-writer-wins on collection
/// (see [`SnapshotRegistry::run`]).
#[derive(Default)]
pub struct SnapshotRegistry {
    projections: Vec<(String, ProjectionFn)>,
}

impl SnapshotRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a projection closure under `key`.
    ///
    /// `key` is the host-chosen snapshot namespace (e.g. `"market.listings"`).
    /// Registering the same key twice keeps both entries; [`Self::run`]
    /// collapses them last-writer-wins, so the most recent registration wins.
    pub fn register(
        &mut self,
        key: impl Into<String>,
        f: impl Fn() -> serde_json::Value + Send + Sync + 'static,
    ) {
        self.projections.push((key.into(), Box::new(f)));
    }

    /// Run every registered projection and collect the results into the map
    /// that becomes [`KernelSnapshot::projections`](super::types::KernelSnapshot).
    ///
    /// D8: this is called on the actor thread inside `make_update`; each
    /// closure must be non-blocking. Empty when nothing is registered — the
    /// snapshot then `skip_serializing_if`s the `projections` key entirely.
    pub fn run(&self) -> HashMap<String, serde_json::Value> {
        let mut out = HashMap::with_capacity(self.projections.len());
        for (key, projection) in &self.projections {
            out.insert(key.clone(), projection());
        }
        out
    }

}

/// Shared snapshot-projection registry handle.
///
/// One `Arc` clone lives on [`NmpApp`](crate::ffi::NmpApp); another is
/// threaded to the actor thread and bound onto the kernel via
/// [`Kernel::set_snapshot_projection_handle`]. Registrations made through the
/// `NmpApp` clone are visible to the kernel without crossing the FFI boundary
/// on each tick — the same shared-`Arc` pattern as the kernel event observer
/// slot.
pub type SnapshotProjectionSlot = Arc<Mutex<SnapshotRegistry>>;

/// Construct a fresh, empty [`SnapshotProjectionSlot`].
pub fn new_snapshot_projection_slot() -> SnapshotProjectionSlot {
    Arc::new(Mutex::new(SnapshotRegistry::new()))
}

impl Kernel {
    /// Install the actor's shared snapshot-projection slot.
    ///
    /// The `Arc<Mutex<…>>` is shared with the FFI surface
    /// (`ffi/snapshot.rs`) and any per-app crate that registered a
    /// projection; the same registrations are therefore visible to both the
    /// actor thread and external Rust callers. Idempotent — re-binding
    /// replaces the prior handle. The actor calls this once immediately after
    /// constructing a kernel.
    pub(crate) fn set_snapshot_projection_handle(&mut self, handle: SnapshotProjectionSlot) {
        self.snapshot_projections = Some(handle);
    }

    /// Extract the snapshot-projection handle before a `Reset` replaces the
    /// kernel. The slot's `Arc<Mutex<…>>` is shared with the FFI surface and
    /// per-app crates, so it MUST survive `Reset` — otherwise every host
    /// projection would silently stop appearing (the same survival contract
    /// as the event observer slot).
    pub(crate) fn take_snapshot_projection_handle_for_reset(
        &mut self,
    ) -> Option<SnapshotProjectionSlot> {
        self.snapshot_projections.take()
    }

    /// Run every registered snapshot projection and return the namespaced
    /// map appended to `KernelSnapshot::projections`.
    ///
    /// Empty (no allocation past the empty map) when no slot is bound, the
    /// mutex is poisoned, or nothing is registered — D6: a projection
    /// failure is data, never a panic at the boundary. Called from
    /// `make_update`.
    pub(in crate::kernel) fn run_snapshot_projections(
        &self,
    ) -> HashMap<String, serde_json::Value> {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|registry| registry.run())
                .unwrap_or_default(),
            None => HashMap::new(),
        }
    }
}
