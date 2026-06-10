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
/// `typed_projections` sidecar (ADR-0037). `nmp-core` never interprets those
/// bytes — the closure (owned by an app/protocol crate) encodes its own typed
/// schema and tags it with `schema_id` / `schema_version` / `file_identifier`.
///
/// Returns `None` when the projection has nothing to emit this tick, so the
/// sidecar omits the entry entirely rather than carrying an empty payload.
///
/// `Send + Sync` because the box lives behind an `Arc<Mutex<…>>` shared with
/// the actor thread (D8: the closure itself must also be non-blocking — it runs
/// inside the snapshot tick, exactly like a generic projection).
pub type TypedProjectionFn = Box<dyn Fn() -> Option<TypedProjectionData> + Send + Sync + 'static>;

/// A host-registered **per-tick observer** closure — a no-result callback fired
/// once on every snapshot tick.
///
/// Unlike a [`ProjectionFn`] / [`TypedProjectionFn`] (which produce snapshot
/// *data* under a key), a tick observer produces nothing: it is a pure per-tick
/// side-effect seam for host-side reconcilers that need a "the kernel just
/// ticked" callback but contribute no projection output (e.g. an active-account
/// subscription reconciler that diffs the active pubkey each tick and enqueues
/// `PushInterest` / `WithdrawInterest` actor commands). Such reconcilers
/// previously abused the projection registry — registering a `ProjectionFn` that
/// returned `Value::Null` purely to get the per-tick callback, leaving a phantom
/// null-valued key in every snapshot.
///
/// `Send + Sync` because the box lives behind an `Arc<Mutex<…>>` shared with the
/// actor thread. D8: like a projection closure, it runs inside the snapshot tick
/// and MUST be non-blocking — it may only enqueue work, never do I/O or wait on
/// a lock.
pub type TickObserverFn = Box<dyn Fn() + Send + Sync + 'static>;

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
    /// Per-tick observers — no-result callbacks fired once per snapshot tick.
    ///
    /// A `Vec` rather than a keyed map: tick observers contribute no snapshot
    /// data, so there is no namespace to collide on and no "replace by key"
    /// semantics — each registration is an independent side-effect that should
    /// fire on every tick. (Production wires exactly one today, the re-homed
    /// zap-subscription reconciler.)
    tick_observers: Vec<TickObserverFn>,
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

    /// Drop the projection(s) registered under `key` from BOTH the generic
    /// and typed registries.
    ///
    /// Used by transient feeds (a visited profile / open thread) whose
    /// snapshot key must not outlive the screen: without this, the
    /// `register_feed`-installed closure keeps running on every 4 Hz tick and
    /// emits an empty subtree under a stale key forever (a leak — both wasted
    /// CPU and a phantom key in every `KernelSnapshot`). Removing from both
    /// maps keeps the generic/typed key space symmetric (a feed may have
    /// registered a typed sidecar alongside its generic projection). Returns
    /// `true` when at least one map held the key. Absent keys are a no-op.
    pub fn remove(&mut self, key: &str) -> bool {
        let removed_generic = self.projections.remove(key).is_some();
        let removed_typed = self.typed_projections.remove(key).is_some();
        removed_generic || removed_typed
    }

    /// Register a **typed** projection closure under `key` — the
    /// FlatBuffers-sidecar counterpart to [`Self::register`].
    ///
    /// `key` is the same host-chosen snapshot namespace used by [`Self::register`]
    /// (e.g. `"nmp.feed.home"`); the typed and generic registries share the key
    /// space so a host can choose, per key, whether to read the typed sidecar or
    /// fall back to the generic `Value` subtree (ADR-0037 Commitment 4).
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

    /// Register a per-tick observer closure — a no-result callback fired once
    /// on every snapshot tick.
    ///
    /// The generic, projection-free counterpart to [`Self::register`]: where a
    /// projection produces snapshot data under a key, a tick observer produces
    /// nothing — it is a pure per-tick side-effect seam (see [`TickObserverFn`]).
    /// Registrations are additive (no key, no replace-by-key); each fires on
    /// every tick. D8: the closure runs inside the snapshot tick and MUST be
    /// non-blocking.
    pub fn register_tick_observer(&mut self, f: impl Fn() + Send + Sync + 'static) {
        self.tick_observers.push(Box::new(f));
    }

    /// Run every registered per-tick observer.
    ///
    /// Mirrors [`Self::run`]'s safety contract: each observer runs on the actor
    /// thread inside `make_update`, so it must be non-blocking (D8). D6: each
    /// observer is invoked inside [`catch_unwind`] — a host tick observer is
    /// untrusted plugin code, and a panic here would otherwise unwind the actor
    /// thread into a terminal `Panic` frame and permanently kill the kernel. A
    /// panicking observer is swallowed (the default panic hook still prints the
    /// payload, so the bug stays visible) and every sibling observer in the same
    /// tick still fires.
    pub fn run_tick_observers(&self) {
        for observer in &self.tick_observers {
            // `AssertUnwindSafe`: a boxed `Fn` closure is not `UnwindSafe`, but a
            // panic here is fully contained — nothing the closure touched is
            // observed again after it unwinds, so there is no broken-invariant
            // hazard.
            let _ = catch_unwind(AssertUnwindSafe(observer));
        }
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
    /// carried in the snapshot frame's `typed_projections` sidecar (ADR-0037).
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

    /// Fire every registered per-tick observer.
    ///
    /// A no-op when no slot is bound or the mutex is poisoned — D6: an observer
    /// dispatch failure is silently absorbed, never a panic at the boundary.
    /// Shares the slot (and therefore the registry) with
    /// [`Self::run_snapshot_projections`]; called from `make_update` on every
    /// tick. The per-observer `catch_unwind` (D6) lives in
    /// [`SnapshotRegistry::run_tick_observers`].
    pub(in crate::kernel) fn run_tick_observers(&self) {
        if let Some(slot) = &self.snapshot_projections {
            if let Ok(registry) = slot.lock() {
                registry.run_tick_observers();
            }
        }
    }
}
