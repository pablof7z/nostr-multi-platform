//! Kernel-side accessors over the shared [`SnapshotProjectionSlot`].
//!
//! Extracted from `snapshot_registry.rs` to keep that file within its LOC
//! ceiling. These are the methods `make_update` (and the `Reset` dispatch arm)
//! call to read the host-extensible registry through the `Arc<Mutex<…>>` slot the
//! actor binds onto the kernel: the generic + typed projection runs, the per-tick
//! observers, and — ADR-0053 — the host-declared consumed-projection set.

use std::collections::HashMap;

use super::super::Kernel;
use super::{DeclaredProjections, SnapshotProjectionSlot};
use crate::update_envelope::TypedProjectionData;

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
    /// projection (and the declared set) would silently stop appearing (the same
    /// survival contract as the event observer slot).
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
    /// [`SnapshotRegistry::run_tick_observers`](super::SnapshotRegistry::run_tick_observers).
    pub(in crate::kernel) fn run_tick_observers(&self) {
        if let Some(slot) = &self.snapshot_projections {
            if let Ok(registry) = slot.lock() {
                registry.run_tick_observers();
            }
        }
    }

    /// ADR-0053 — snapshot the host-declared consumed-projection set for this
    /// tick.
    ///
    /// Cloned ONCE at the top of `snapshot_projections_with_publish_cluster` so
    /// the per-key `permits()` checks don't re-lock the registry mutex for every
    /// Tier-2 built-in. When no slot is bound or the mutex is poisoned the result
    /// is an empty `DeclaredProjections` — which `permits()` everything (D6: a
    /// gate-read failure degrades to "no narrowing", never to "drop all
    /// built-ins", and never a panic at the boundary).
    pub(in crate::kernel) fn declared_projections_snapshot(&self) -> DeclaredProjections {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|registry| registry.declared_projections().clone())
                .unwrap_or_default(),
            None => DeclaredProjections::default(),
        }
    }

    /// ADR-0055 Rung 3 — read whether the host has declared incremental-apply
    /// capability for this kernel instance.
    ///
    /// Called once per tick in `make_update` to determine whether to pass
    /// `enabled = true` to `rung3_omit::omit_unchanged`. When no slot is bound
    /// or the mutex is poisoned the result is `false` — "full rows" (D6: a
    /// gate-read failure degrades to safe behavior, never to data loss).
    pub(in crate::kernel) fn incremental_apply_enabled(&self) -> bool {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|registry| registry.is_incremental_apply_enabled())
                .unwrap_or(false),
            None => false,
        }
    }

    /// ADR-0055 Rung 3 (D3-5) — take the "baseline pending" latch.
    ///
    /// Returns `true` exactly once after `declare_incremental_apply` sets the
    /// latch, clearing it atomically. The caller (`make_update`) MUST then call
    /// `self.projection_rev_tracker.reset_last_emitted()` so the next frame is
    /// a guaranteed full baseline for the newly-attached incremental host.
    /// When no slot is bound or the mutex is poisoned, returns `false` (D6:
    /// degrades safely to "no reset needed" — the tracker's initial state is
    /// already a full baseline).
    pub(in crate::kernel) fn take_incremental_apply_baseline_pending(&mut self) -> bool {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|mut registry| registry.take_incremental_apply_baseline_pending())
                .unwrap_or(false),
            None => false,
        }
    }
}
