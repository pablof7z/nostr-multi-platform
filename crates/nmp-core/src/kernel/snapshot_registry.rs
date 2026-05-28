//! Host-extensible snapshot output — the `nmp_app_register_snapshot_projection`
//! seam.
//!
//! This is the output-side counterpart to the action-registry seam
//! (`ActionRegistry::register::<M>()`). Where the action registry lets a host
//! *dispatch* a custom namespace, the snapshot registry lets a host *project*
//! a custom namespace into the snapshot every tick emits.
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
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

use super::Kernel;
use crate::update_envelope::TypedProjectionData;

/// A host-registered projection closure.
///
/// Takes no arguments — a snapshot tick is a pull, the kernel drives it — and
/// returns the JSON value to append under the registered key. `Send + Sync`
/// because the box lives behind an `Arc<Mutex<…>>` shared with the actor
/// thread (D8: the closure itself must also be non-blocking).
pub type ProjectionFn = Box<dyn Fn() -> serde_json::Value + Send + Sync + 'static>;

/// A host-registered **typed** projection closure — the FlatBuffers-sidecar
/// counterpart to [`ProjectionFn`].
///
/// Where a [`ProjectionFn`] returns a generic `serde_json::Value` appended to
/// `KernelSnapshot::projections`, a `TypedProjectionFn` returns opaque
/// FlatBuffers bytes ([`TypedProjectionData`]) carried in the snapshot frame's
/// `typed_projections` sidecar (ADR-0035). `nmp-core` never interprets those
/// bytes — the closure (owned by an app/protocol crate) encodes its own typed
/// schema and tags it with `schema_id` / `schema_version` / `file_identifier`.
///
/// Returns `None` when the projection has nothing to emit this tick, so the
/// sidecar omits the entry entirely rather than carrying an empty payload.
///
/// `Send + Sync` because the box lives behind an `Arc<Mutex<…>>` shared with
/// the actor thread (D8: the closure itself must also be non-blocking — it runs
/// inside the snapshot tick, exactly like a generic projection).
pub type TypedProjectionFn =
    Box<dyn Fn() -> Option<TypedProjectionData> + Send + Sync + 'static>;

/// Registry of host-supplied snapshot projections.
///
/// Keyed by `String` so re-registering the same key replaces the old closure
/// rather than appending a duplicate. This prevents CPU waste: a re-registered
/// projection previously caused both the old and new closures to run on every
/// snapshot tick, with only the last result surfacing in the output.
#[derive(Default)]
pub struct SnapshotRegistry {
    projections: HashMap<String, ProjectionFn>,
    typed_projections: HashMap<String, TypedProjectionFn>,
}

impl SnapshotRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a projection closure under `key`.
    ///
    /// `key` is the host-chosen snapshot namespace (e.g. `"market.listings"`).
    /// Registering the same key twice replaces the first — last-writer-wins,
    /// with no duplicate-closure CPU cost on subsequent ticks.
    pub fn register(
        &mut self,
        key: impl Into<String>,
        f: impl Fn() -> serde_json::Value + Send + Sync + 'static,
    ) {
        self.projections.insert(key.into(), Box::new(f));
    }

    /// Run every registered projection and collect the results into the map
    /// that becomes [`KernelSnapshot::projections`](super::types::KernelSnapshot).
    ///
    /// D8: this is called on the actor thread inside `make_update`; each
    /// closure must be non-blocking. Empty when nothing is registered — the
    /// snapshot then `skip_serializing_if`s the `projections` key entirely.
    ///
    /// D6: each host closure is invoked inside [`catch_unwind`] — a host
    /// projection is untrusted plugin code, and this runs on the actor
    /// thread *inside* the snapshot tick. An unguarded panic would unwind
    /// the actor thread; the actor's outer `catch_unwind` would then catch a
    /// terminal `Panic` frame and the kernel would be permanently dead. A
    /// panicking projection MUST never be able to kill the kernel: its key
    /// is omitted from the map (the same shape as an unregistered
    /// namespace), and every sibling projection in the same tick still
    /// produces its value.
    pub fn run(&self) -> HashMap<String, serde_json::Value> {
        let mut out = HashMap::with_capacity(self.projections.len());
        for (key, projection) in &self.projections {
            // `AssertUnwindSafe`: a boxed `Fn` closure is not `UnwindSafe`,
            // but a panic here is fully contained — nothing the closure
            // touched is observed again after it unwinds, so there is no
            // broken-invariant hazard.
            match catch_unwind(AssertUnwindSafe(projection)) {
                Ok(value) => {
                    out.insert(key.clone(), value);
                }
                // The panic is swallowed: the namespace is omitted, exactly
                // as if the host had never registered it. The default panic
                // hook still prints the payload, so the bug stays visible.
                Err(_) => continue,
            }
        }
        out
    }

    /// Register a **typed** projection closure under `key` — the
    /// FlatBuffers-sidecar counterpart to [`Self::register`].
    ///
    /// `key` is the same host-chosen snapshot namespace used by [`Self::register`]
    /// (e.g. `"nmp.feed.home"`); the typed and generic registries share the key
    /// space so a host can choose, per key, whether to read the typed sidecar or
    /// fall back to the generic `Value` subtree (ADR-0035 Commitment 4).
    /// Registering the same key twice replaces the first — last-writer-wins, with
    /// no duplicate-closure CPU cost on subsequent ticks.
    pub fn register_typed(
        &mut self,
        key: impl Into<String>,
        f: impl Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    ) {
        self.typed_projections.insert(key.into(), Box::new(f));
    }

    /// Run every registered typed projection and collect the results into the
    /// vector that becomes the snapshot frame's `typed_projections` sidecar.
    ///
    /// Mirrors [`Self::run`]: each closure runs on the actor thread inside
    /// `make_update`, so it must be non-blocking (D8). A closure that returns
    /// `None` contributes no sidecar entry (nothing to emit this tick); a
    /// closure that panics is swallowed inside [`catch_unwind`] (D6) and its key
    /// is omitted, exactly as if it had never been registered — every sibling
    /// projection in the same tick still produces its value, and a panicking
    /// host projection can never unwind the actor thread into a terminal
    /// `Panic` frame.
    pub fn run_typed(&self) -> Vec<TypedProjectionData> {
        let mut out = Vec::with_capacity(self.typed_projections.len());
        for projection in self.typed_projections.values() {
            // `AssertUnwindSafe`: a boxed `Fn` closure is not `UnwindSafe`, but
            // a panic here is fully contained — nothing the closure touched is
            // observed again after it unwinds, so there is no broken-invariant
            // hazard. The default panic hook still prints the payload, so the
            // bug stays visible.
            match catch_unwind(AssertUnwindSafe(projection)) {
                Ok(Some(data)) => out.push(data),
                // `Ok(None)`: nothing to emit this tick. `Err(_)`: the closure
                // panicked — swallow it (the namespace is omitted, the same
                // shape as an unregistered projection).
                Ok(None) | Err(_) => continue,
            }
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
#[must_use]
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
    pub(in crate::kernel) fn run_snapshot_projections(&self) -> HashMap<String, serde_json::Value> {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|registry| registry.run())
                .unwrap_or_default(),
            None => HashMap::new(),
        }
    }

    /// Run every registered **typed** snapshot projection and return the vector
    /// carried in the snapshot frame's `typed_projections` sidecar (ADR-0035).
    ///
    /// Empty when no slot is bound, the mutex is poisoned, or nothing is
    /// registered — D6: a projection failure is data, never a panic at the
    /// boundary. Shares the slot (and therefore the registry) with
    /// [`Self::run_snapshot_projections`]; called from `make_update`.
    pub(in crate::kernel) fn run_typed_projections(&self) -> Vec<TypedProjectionData> {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|registry| registry.run_typed())
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }
}
