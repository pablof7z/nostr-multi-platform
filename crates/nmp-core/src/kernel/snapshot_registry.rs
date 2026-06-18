//! Host-extensible snapshot output — the typed FlatBuffers sidecar seam
//! (ADR-0037).
//!
//! Hosts register typed projection closures whose FlatBuffers bytes are
//! carried in the snapshot frame's `typed_projections` sidecar.  The legacy
//! generic (`serde_json::Value`) lane has been removed; only typed projections
//! remain.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use crate::update_envelope::{TypedProjectionData, WireProjectionState};

/// A host-registered **typed** projection closure — the FlatBuffers-sidecar
/// counterpart to the removed generic `ProjectionFn`.
///
/// Where the old `ProjectionFn` returned a generic `serde_json::Value` appended to
/// `KernelSnapshot::projections`, a `TypedProjectionFn` returns opaque
/// FlatBuffers bytes ([`TypedProjectionData`]) carried in the snapshot frame's
/// `typed_projections` sidecar (ADR-0037). `nmp-core` never interprets those
/// bytes — the closure (owned by an app/protocol crate) encodes its own typed
/// schema and tags it with `schema_id` / `schema_version` / `file_identifier`.
///
/// Returns `None` when the projection has no changed payload this tick. Under
/// incremental apply, omission means the host retains its cached value; it is
/// never a clear signal. To clear a registered typed key, remove it through
/// [`SnapshotRegistry::remove`], which emits a one-shot `Cleared` row.
///
/// `Send + Sync` because the box lives behind an `Arc<Mutex<…>>` shared with
/// the actor thread (D8: the closure itself must also be non-blocking — it runs
/// inside the snapshot tick, exactly like a generic projection).
pub type TypedProjectionFn = Box<dyn Fn() -> Option<TypedProjectionData> + Send + Sync + 'static>;

/// A host-registered **per-tick observer** closure — a no-result callback fired
/// once on every snapshot tick.
///
/// Unlike a [`TypedProjectionFn`] (which produces snapshot
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

/// Result of a [`SnapshotRegistry::register_typed`] call (Blocker C).
///
/// Callers that record a composition-ledger disposition MUST derive it from
/// this result rather than from a pre-insertion key-presence check, so the
/// ledger stays truthful when the registry is at the D5 cap and silently drops
/// a new-key registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedAdmission {
    /// A new key was accepted and the closure inserted.
    Inserted,
    /// An existing key was replaced (always allowed regardless of the cap).
    Replaced,
    /// A new key was rejected because the registry is at the
    /// [`bounds::MAX_SNAPSHOT_PROJECTIONS`] ceiling (D5 loud no-op).
    DroppedFull,
}

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
    typed_projections: HashMap<String, TypedProjectionFn>,
    /// One-shot `Cleared` rows for host-registered typed projections removed from
    /// the registry. Tier-1 keys are not in the built-in manifest, so Rung 3
    /// cannot synthesize these clears from `ProjectionPresence`.
    pending_typed_clears: Vec<String>,
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
    /// set-before-start) via [`declare_incremental_apply`]. Durable architecture
    /// (per-attach baseline gate + Rung-5 ADR-0053 compose seam), NOT a compat
    /// shim — deleted only when every NMP host advertises it unconditionally.
    ///
    /// **Single source of truth (R6-S1).** This `Arc<AtomicBool>` is THE flag;
    /// `make_update` reads it lock-free and a Tier-1 producer captures a clone
    /// via [`Self::incremental_apply_handle`] — same atomic, no mirror anywhere.
    incremental_apply_enabled: Arc<AtomicBool>,
    /// ADR-0055 Rung 3 (D3-5) — one-shot latch set by `declare_incremental_apply`.
    ///
    /// The kernel reads and clears this in `make_update` (via
    /// `take_incremental_apply_baseline_pending`) and calls
    /// `ProjectionRevTracker::reset_last_emitted` when `true`, guaranteeing
    /// the next frame is a full baseline for the newly-declared host.
    incremental_apply_baseline_pending: bool,
    /// ADR-0055 R6-S1 — frame identity for Tier-1 producers that omit unchanged
    /// frames. `session_id` = `started_unix_ms` (changes on `Reset`-rebuild);
    /// `snapshot_epoch` = `tracker.epoch`. Written each tick by
    /// `Kernel::publish_frame_identity` before the typed projections run; a
    /// producer captures clones via [`Self::frame_identity_handles`] and
    /// rebaselines when EITHER differs from the last tuple — the SAME signal the
    /// host `ProjectionCache` resets on (the freeze fix). Lives here because the
    /// registry survives `Reset`.
    frame_session_id: Arc<AtomicU64>,
    frame_snapshot_epoch: Arc<AtomicU64>,
}

use std::collections::HashMap;

impl SnapshotRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the projection(s) registered under `key` from the typed registry.
    ///
    /// Used by transient feeds (a visited profile / open thread) whose
    /// snapshot key must not outlive the screen: without this, the
    /// `register_feed`-installed closure keeps running on every 4 Hz tick and
    /// emits an empty subtree under a stale key forever (a leak — both wasted
    /// CPU and a phantom key in every `KernelSnapshot`). Returns
    /// `true` when the typed map held the key. Absent keys are a no-op.
    pub fn remove(&mut self, key: &str) -> bool {
        let removed_typed = self.typed_projections.remove(key).is_some();
        let clear_pending = self
            .pending_typed_clears
            .iter()
            .any(|pending| pending == key);
        if removed_typed && !clear_pending {
            self.pending_typed_clears.push(key.to_string());
        }
        removed_typed
    }

    /// Return the set of typed projection keys currently registered in the
    /// registry — without running any closures.
    ///
    /// Intended for coverage-gate tests that need to verify that a key was
    /// registered (regardless of whether its closure would return `Some` for
    /// the current state). Production code should use [`Self::run_typed`].
    #[must_use]
    pub fn registered_typed_keys(&self) -> impl Iterator<Item = &str> {
        self.typed_projections.keys().map(|k| k.as_str())
    }

    /// Register a **typed** projection closure under `key` — the
    /// FlatBuffers-sidecar seam (ADR-0037).
    ///
    /// `key` is the host-chosen snapshot namespace (e.g. `"nmp.feed.home"`).
    /// Registering the same key twice replaces the first — last-writer-wins, with
    /// no duplicate-closure CPU cost on subsequent ticks.
    ///
    /// D5: same [`MAX_SNAPSHOT_PROJECTIONS`](bounds::MAX_SNAPSHOT_PROJECTIONS) ceiling;
    /// re-registering an existing key is always allowed.
    ///
    /// Returns a [`TypedAdmission`] describing what actually happened so the caller
    /// can record a truthful composition-ledger disposition (Blocker C):
    /// - [`TypedAdmission::Inserted`] — new key accepted (was absent, below the cap).
    /// - [`TypedAdmission::Replaced`] — existing key replaced (always allowed).
    /// - [`TypedAdmission::DroppedFull`] — new key rejected (registry is at the
    ///   [`MAX_SNAPSHOT_PROJECTIONS`](bounds::MAX_SNAPSHOT_PROJECTIONS) ceiling).
    pub fn register_typed(
        &mut self,
        key: impl Into<String>,
        f: impl Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    ) -> TypedAdmission {
        let key = key.into();
        let key_exists = self.typed_projections.contains_key(&key);
        if !admit_keyed(
            self.typed_projections.len(),
            key_exists,
            &key,
            "typed snapshot projection",
        ) {
            return TypedAdmission::DroppedFull;
        }
        self.pending_typed_clears.retain(|pending| pending != &key);
        self.typed_projections.insert(key, Box::new(f));
        if key_exists {
            TypedAdmission::Replaced
        } else {
            TypedAdmission::Inserted
        }
    }

    /// Run every registered typed projection and collect the results into the
    /// vector that becomes the snapshot frame's `typed_projections` sidecar.
    ///
    /// Mirrors the removed generic `run`: each closure runs on the actor thread inside
    /// `make_update`, so it must be non-blocking (D8). `None` means retain the
    /// prior value; removed keys emit one pending `Cleared` row. Closure panics
    /// are swallowed inside [`catch_unwind`] (D6).
    pub fn run_typed(&mut self) -> Vec<TypedProjectionData> {
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
        out.extend(
            std::mem::take(&mut self.pending_typed_clears)
                .into_iter()
                .map(|key| TypedProjectionData {
                    key,
                    state: WireProjectionState::Cleared,
                    ..Default::default()
                }),
        );
        out
    }

    /// Register a per-tick observer closure — a no-result callback fired once
    /// on every snapshot tick.
    ///
    /// The generic, projection-free counterpart to [`Self::register_typed`]: where a
    /// typed projection produces snapshot data under a key, a tick observer produces
    /// nothing — it is a pure per-tick side-effect seam (see [`TickObserverFn`]).
    /// Registrations are additive (no key, no replace-by-key); each fires on
    /// every tick. D8: the closure runs inside the snapshot tick and MUST be
    /// non-blocking.
    ///
    /// D5: if the observer list already holds [`MAX_TICK_OBSERVERS`](bounds::MAX_TICK_OBSERVERS) entries the
    /// registration is a loud no-op (D6: `tracing::warn!`, no panic).
    pub fn register_tick_observer(&mut self, f: impl Fn() + Send + Sync + 'static) {
        if !admit_additive(self.tick_observers.len()) {
            return;
        }
        self.tick_observers.push(Box::new(f));
    }

    // ADR-0053 host-declared consumed-projection methods
    // (`declare_consumed_projections`, `declared_projections`) and the
    // Workstream-E3 declared ⊆ decodable drift gate live in the `declared`
    // submodule alongside `DeclaredProjections`.

    /// Run every registered per-tick observer.
    ///
    /// Mirrors [`Self::run_typed`]'s safety contract: each observer runs on the actor
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

// Kernel-side accessors over the shared slot (set/take handle, run typed
// projections, run tick observers, ADR-0053 declared-set snapshot) live in
// the `kernel_access` submodule to keep this file within its LOC ceiling.
mod kernel_access;

// ADR-0055 Rung 3 — the `declare_incremental_apply` / `is_incremental_apply_enabled`
// / `take_incremental_apply_baseline_pending` inherent methods live in the
// `incremental_apply` submodule to keep this file within its LOC ceiling. The
// two backing fields remain on the struct definition above.
mod incremental_apply;
