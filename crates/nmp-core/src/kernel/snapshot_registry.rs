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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::update_envelope::TypedProjectionData;

/// A host-registered projection closure.
///
/// Takes no arguments — a snapshot tick is a pull, the kernel drives it — and
/// returns the JSON value to append under the registered key. `Send + Sync`
/// because the box lives behind an `Arc<Mutex<…>>` shared with the actor
/// thread (D8: the closure itself must also be non-blocking).
pub type ProjectionFn = Box<dyn Fn() -> serde_json::Value + Send + Sync + 'static>;

/// A monotonic **change gate** for a snapshot projection.
///
/// The defect this exists to fix: [`SnapshotRegistry::run`] previously called
/// *every* registered projection closure on *every* `make_update`, with no
/// per-projection change tracking. A multi-MB library serializer therefore
/// re-ran on every unrelated kernel emit (an incoming relay event, a tick),
/// pegging the actor thread on JSON serialization it could have skipped.
///
/// A gate lets a host declare "my inputs only changed when this counter
/// advanced." The host bumps the counter (via its own shared `Arc<AtomicU64>`
/// rev) whenever the projection's source data mutates. The registry remembers
/// the last gate value it witnessed per key alongside the last value the closure
/// produced; on the next `run`, if the gate value is unchanged, the registry
/// returns the cached value WITHOUT invoking the closure (see
/// [`SnapshotRegistry::register_gated`]).
///
/// The canonical gate is an [`AtomicU64`] rev counter — most consuming apps
/// already maintain exactly such a rev — so [`AtomicU64`] implements this trait
/// directly and an `Arc<AtomicU64>` can be passed as the gate. Custom gates
/// (e.g. a content hash collapsed into a `u64`) implement the trait themselves.
///
/// `Send + Sync` because the gate is shared between the host (which bumps it)
/// and the actor thread (which reads it through the registry).
pub trait ChangeGate: Send + Sync + 'static {
    /// The current gate value. A change in this value (relative to the value
    /// witnessed on the previous `run`) marks the projection dirty; an
    /// unchanged value lets the registry serve the cached projection output.
    fn current(&self) -> u64;
}

impl ChangeGate for AtomicU64 {
    fn current(&self) -> u64 {
        self.load(Ordering::Acquire)
    }
}

// `ProjectionEntry` — the per-key closure + change-gate memo machinery,
// extracted to a submodule to keep this file within its LOC ceiling.
mod entry;
use entry::ProjectionEntry;

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

// D5 — registration-count ceilings and the loud-no-op admission helpers
// (`MAX_SNAPSHOT_PROJECTIONS` / `MAX_TICK_OBSERVERS` + `admit_keyed` /
// `admit_additive`). Extracted to a `pub` submodule so the registry file stays
// within its LOC ceiling; the constants are part of the public D5 contract.
pub mod bounds;
use bounds::{admit_additive, admit_keyed};

// ADR-0053 — the host-declared consumed-projection set. Extracted to a `pub`
// submodule so the registry file stays within its LOC ceiling; the type is part
// of the public seam (read by the kernel to gate Tier-2 built-ins).
pub mod declared;
pub use declared::DeclaredProjections;

// ADR-0053 — end-to-end gating proofs. Mounted here (not from `kernel/mod.rs`)
// via `#[path]` so the kernel god-module stays at its size baseline. The test
// file uses absolute `crate::kernel::` paths so the mount point is irrelevant.
#[cfg(test)]
#[path = "declared_projections_tests.rs"]
mod declared_projections_tests;

// D5 — registration-count ceiling tests (kept beside the registry, off the
// `kernel/mod.rs` module list, so this PR does not touch that ratcheted file).
#[cfg(test)]
mod bounds_tests;

/// Registry of host-supplied snapshot projections.
///
/// Keyed by `String` so re-registering the same key replaces the old closure
/// rather than appending a duplicate. This prevents CPU waste: a re-registered
/// projection previously caused both the old and new closures to run on every
/// snapshot tick, with only the last result surfacing in the output.
#[derive(Default)]
pub struct SnapshotRegistry {
    projections: HashMap<String, ProjectionEntry>,
    typed_projections: HashMap<String, TypedProjectionFn>,
    /// Per-tick observers — no-result callbacks fired once per snapshot tick.
    ///
    /// A `Vec` rather than a keyed map: tick observers contribute no snapshot
    /// data, so there is no namespace to collide on and no "replace by key"
    /// semantics — each registration is an independent side-effect that should
    /// fire on every tick. (Production wires exactly one today, the re-homed
    /// zap-subscription reconciler.)
    tick_observers: Vec<TickObserverFn>,
    /// ADR-0053 — the host-declared set of consumed Tier-2 built-in projection
    /// keys. Empty (the default) means "no opinion / no narrowing" — every
    /// Tier-2 built-in is emitted, as before this ADR. A non-empty set narrows
    /// the kernel-owned built-ins to its members. Tier-1 host/protocol
    /// projections are unaffected (they self-gate by registration). See
    /// [`DeclaredProjections`].
    declared_projections: DeclaredProjections,
    /// ADR-0055 Rung 3 — the host-declared incremental-apply capability.
    ///
    /// `false` (the default) means "full rows every tick" — the kernel emits
    /// the complete typed sidecar on every `make_update`, unchanged from Rung 2.
    ///
    /// `true` means the host runtime owns the NMP cache-merge layer (D3-3) and
    /// the kernel is permitted to omit `Unchanged` projections from the frame.
    /// The host MUST set this before `nmp_app_start` (single-writer,
    /// set-before-start) via [`declare_incremental_apply`] /
    /// [`AppHost::declare_incremental_apply`] /
    /// `nmp_app_declare_incremental_apply()`. This is durable architecture (the
    /// per-attach baseline gate + the Rung-5 ADR-0053 compose seam), NOT a
    /// compat shim — it is deleted only when every NMP host advertises it
    /// unconditionally (a future cleanup once Tier-1 gating + Rung 4 land).
    incremental_apply_enabled: bool,
    /// ADR-0055 Rung 3 (D3-5) — one-shot latch set by `declare_incremental_apply`.
    ///
    /// The kernel reads and clears this in `make_update` (via
    /// `take_incremental_apply_baseline_pending`) and calls
    /// `ProjectionRevTracker::reset_last_emitted` when `true`, guaranteeing
    /// the next frame is a full baseline for the newly-declared host.
    incremental_apply_baseline_pending: bool,
}

impl SnapshotRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an **always-run** projection closure under `key`.
    ///
    /// `key` is the host-chosen snapshot namespace (e.g. `"market.listings"`).
    /// Registering the same key twice replaces the first — last-writer-wins,
    /// with no duplicate-closure CPU cost on subsequent ticks.
    ///
    /// The closure runs on **every** `run` (every `make_update` tick). When the
    /// projection serializes a large structure that rarely changes, prefer
    /// [`Self::register_gated`] to skip re-running it on ticks where its inputs
    /// did not change.
    ///
    /// D5: if this is a **new** key and the registry already holds
    /// [`MAX_SNAPSHOT_PROJECTIONS`] entries, the registration is silently
    /// dropped and a `tracing::warn!` diagnostic is emitted (D6: no panic).
    pub fn register(
        &mut self,
        key: impl Into<String>,
        f: impl Fn() -> serde_json::Value + Send + Sync + 'static,
    ) {
        let key = key.into();
        // D5: admit a new key only when below the ceiling; re-registering an
        // existing key is always allowed (it just replaces the closure).
        if !admit_keyed(
            self.projections.len(),
            self.projections.contains_key(&key),
            &key,
            "snapshot projection",
        ) {
            return;
        }
        self.projections
            .insert(key, ProjectionEntry::ungated(Box::new(f)));
    }

    /// Register a **change-gated** projection closure under `key`.
    ///
    /// Identical to [`Self::register`] except the closure is only re-invoked
    /// when `gate`'s value has advanced since the previous `run` for this key.
    /// On a tick where the gate is unchanged, `run` returns the value the
    /// closure last produced — cloned from a per-key memo — WITHOUT calling the
    /// closure. This is the fix for the "re-serialize the whole library on every
    /// emit" hot path (see [`ChangeGate`]).
    ///
    /// The natural `gate` is an `Arc<AtomicU64>` rev counter the host already
    /// maintains, bumped whenever the projection's source data mutates
    /// ([`AtomicU64`] implements [`ChangeGate`]). The first `run` always invokes
    /// the closure (no memo yet) and records the gate value; thereafter an
    /// unchanged gate serves the cache.
    ///
    /// Last-writer-wins by `key`, exactly like [`Self::register`]; re-registering
    /// (gated or ungated) replaces the entry and discards any prior memo.
    ///
    /// D5: same [`MAX_SNAPSHOT_PROJECTIONS`] ceiling as [`Self::register`];
    /// re-registering an existing key is always allowed.
    pub fn register_gated(
        &mut self,
        key: impl Into<String>,
        gate: Arc<dyn ChangeGate>,
        f: impl Fn() -> serde_json::Value + Send + Sync + 'static,
    ) {
        let key = key.into();
        if !admit_keyed(
            self.projections.len(),
            self.projections.contains_key(&key),
            &key,
            "snapshot projection",
        ) {
            return;
        }
        self.projections
            .insert(key, ProjectionEntry::gated(gate, Box::new(f)));
    }

    /// Run every registered projection and collect the results into the map
    /// that becomes [`KernelSnapshot::projections`](super::types::KernelSnapshot).
    ///
    /// D8: this is called on the actor thread inside `make_update`; each
    /// closure must be non-blocking. Empty when nothing is registered — the
    /// snapshot then `skip_serializing_if`s the `projections` key entirely.
    ///
    /// D6/D15: each host closure is invoked inside [`catch_unwind`] inside
    /// [`ProjectionEntry::value_for_tick`] — a host projection is untrusted
    /// plugin code, and this runs on the actor thread *inside* the snapshot
    /// tick. An unguarded panic would unwind the actor thread; the actor's
    /// outer `catch_unwind` would then catch a terminal `Panic` frame and the
    /// kernel would be permanently dead. A panicking projection MUST never be
    /// able to kill the kernel: its key is omitted from the map (the same
    /// shape as an unregistered namespace), and every sibling projection in
    /// the same tick still produces its value.
    pub fn run(&self) -> HashMap<String, serde_json::Value> {
        let mut out = HashMap::with_capacity(self.projections.len());
        for (key, entry) in &self.projections {
            // `value_for_tick` wraps every host-closure invocation in
            // `catch_unwind` (D15) and returns `None` when the closure
            // panicked. The panic is swallowed: the namespace is omitted,
            // exactly as if the host had never registered it. The default
            // panic hook still prints the payload, so the bug stays visible.
            if let Some(value) = entry.value_for_tick() {
                out.insert(key.clone(), value);
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
    ///
    /// D5: same [`MAX_SNAPSHOT_PROJECTIONS`] ceiling as [`Self::register`],
    /// applied independently to the typed registry.  Re-registering an existing
    /// key is always allowed.
    pub fn register_typed(
        &mut self,
        key: impl Into<String>,
        f: impl Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    ) {
        let key = key.into();
        if !admit_keyed(
            self.typed_projections.len(),
            self.typed_projections.contains_key(&key),
            &key,
            "typed snapshot projection",
        ) {
            return;
        }
        self.typed_projections.insert(key, Box::new(f));
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
    ///
    /// D5: if the observer list already holds [`MAX_TICK_OBSERVERS`] entries the
    /// registration is a loud no-op (D6: `tracing::warn!`, no panic).
    pub fn register_tick_observer(&mut self, f: impl Fn() + Send + Sync + 'static) {
        if !admit_additive(self.tick_observers.len()) {
            return;
        }
        self.tick_observers.push(Box::new(f));
    }

    /// ADR-0053 — declare (union into) the set of Tier-2 built-in projection
    /// keys this host consumes.
    ///
    /// Additive: call more than once and the sets union (e.g. a base set from
    /// `nmp-defaults` plus an app-specific extension). Intended as a host-init
    /// call, before `nmp_app_start`. An empty declared set leaves the kernel
    /// emitting every Tier-2 built-in (no narrowing); a non-empty set narrows
    /// the kernel-owned built-ins to the declared members. Tier-1 host/protocol
    /// projections are unaffected — they self-gate by registration.
    pub fn declare_consumed_projections<I, K>(&mut self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        self.declared_projections.declare(keys);
    }

    /// Read the host-declared consumed-projection set — the gate the kernel
    /// consults per Tier-2 built-in key in `make_update`.
    #[must_use]
    pub fn declared_projections(&self) -> &DeclaredProjections {
        &self.declared_projections
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

// Kernel-side accessors over the shared slot (set/take handle, run generic +
// typed projections, run tick observers, ADR-0053 declared-set snapshot) live in
// the `kernel_access` submodule to keep this file within its LOC ceiling.
mod kernel_access;

// ADR-0055 Rung 3 — the `declare_incremental_apply` / `is_incremental_apply_enabled`
// / `take_incremental_apply_baseline_pending` inherent methods live in the
// `incremental_apply` submodule to keep this file within its LOC ceiling. The
// two backing fields remain on the struct definition above.
mod incremental_apply;
