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

use super::Kernel;
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

/// A registered generic projection: the closure plus the optional change-gate
/// memo that lets `run` skip re-invoking the closure when the gate is unchanged.
///
/// - `f` — the host projection closure (always present).
/// - `gate` — `None` for the default always-run registration
///   ([`SnapshotRegistry::register`]); `Some` for the gated variant
///   ([`SnapshotRegistry::register_gated`]).
/// - `memo` — interior-mutable per-key cache of `(last witnessed gate value,
///   last produced value)`. Interior mutability (a `Mutex`) is required because
///   [`SnapshotRegistry::run`] takes `&self` (it is driven from `make_update`
///   through a shared `&self` kernel path); threading `&mut` all the way through
///   `make_update` would ripple a borrow change across the whole emit path. The
///   `Mutex` is contended only by the single actor thread that drives `run`, so
///   it is effectively uncontended in production. `None` until the first run
///   populates it; ignored entirely when `gate` is `None`.
struct ProjectionEntry {
    f: ProjectionFn,
    gate: Option<Arc<dyn ChangeGate>>,
    memo: Mutex<Option<(u64, serde_json::Value)>>,
}

impl ProjectionEntry {
    /// An ungated (always-run) entry — the default registration semantics.
    fn ungated(f: ProjectionFn) -> Self {
        Self {
            f,
            gate: None,
            memo: Mutex::new(None),
        }
    }

    /// A gated entry — `run` consults `gate` and may serve `memo` instead of
    /// invoking `f`.
    fn gated(gate: Arc<dyn ChangeGate>, f: ProjectionFn) -> Self {
        Self {
            f,
            gate: Some(gate),
            memo: Mutex::new(None),
        }
    }

    /// Produce this entry's value for the current tick.
    ///
    /// Ungated: always invokes the closure (legacy semantics, unchanged).
    /// Gated: if the gate value matches the memoized value, clones and returns
    /// the cached value WITHOUT invoking the closure; otherwise invokes the
    /// closure, caches `(gate_value, value)`, and returns it.
    ///
    /// D6: the closure invocation is wrapped in [`catch_unwind`] by the caller
    /// ([`SnapshotRegistry::run`]). The closure runs OUTSIDE the memo lock, so a
    /// panic neither poisons the memo nor pins a stale value — any prior memo is
    /// left intact and the key is simply omitted this tick.
    fn value_for_tick(&self) -> serde_json::Value {
        let Some(gate) = self.gate.as_ref() else {
            // Ungated: the default always-run path — never touches the memo.
            return (self.f)();
        };

        let gate_value = gate.current();
        // Fast path: a clean gate serves the cached value without invoking `f`.
        // The memo mutex is contended only by the single actor thread, so this
        // lock is effectively uncontended. A poisoned memo (defensive — the lock
        // is always released before `f` runs, so a closure panic can never
        // poison it) collapses to a fresh run.
        if let Ok(memo) = self.memo.lock() {
            if let Some((cached_gate, cached_value)) = memo.as_ref() {
                if *cached_gate == gate_value {
                    return cached_value.clone();
                }
            }
        }

        // Dirty (or never run): invoke `f`, then memoize for the next tick. `f`
        // runs OUTSIDE the memo lock so a slow/panicking closure never holds the
        // memo mutex.
        let value = (self.f)();
        if let Ok(mut memo) = self.memo.lock() {
            *memo = Some((gate_value, value.clone()));
        }
        value
    }
}

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
    pub fn register(
        &mut self,
        key: impl Into<String>,
        f: impl Fn() -> serde_json::Value + Send + Sync + 'static,
    ) {
        self.projections
            .insert(key.into(), ProjectionEntry::ungated(Box::new(f)));
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
    pub fn register_gated(
        &mut self,
        key: impl Into<String>,
        gate: Arc<dyn ChangeGate>,
        f: impl Fn() -> serde_json::Value + Send + Sync + 'static,
    ) {
        self.projections
            .insert(key.into(), ProjectionEntry::gated(gate, Box::new(f)));
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
        for (key, entry) in &self.projections {
            // `AssertUnwindSafe`: a boxed `Fn` closure is not `UnwindSafe`,
            // but a panic here is fully contained — nothing the closure
            // touched is observed again after it unwinds, so there is no
            // broken-invariant hazard. `value_for_tick` runs the gate check
            // and either serves the cached value (gated, clean) or invokes the
            // closure; the panic boundary wraps the whole thing so a panic in
            // the closure leaves the per-key memo untouched and omits the key.
            match catch_unwind(AssertUnwindSafe(|| entry.value_for_tick())) {
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
