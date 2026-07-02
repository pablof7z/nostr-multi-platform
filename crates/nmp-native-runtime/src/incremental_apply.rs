//! ADR-0070 Rung 3 / R6-S1 — incremental-apply capability + frame-identity
//! accessors on [`NmpApp`].
//!
//! Split out of `lib.rs` (file-size discipline) as a cohesive `impl NmpApp`
//! block. All three methods read/write through the shared `SnapshotRegistry`
//! slot (`self.snapshot_projections`), which is the SINGLE source of truth for
//! the incremental-apply flag and the home of the frame-identity handles the
//! kernel publishes each tick. There is no `NmpApp`-side mirror state.

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

use super::NmpApp;

impl NmpApp {
    /// ADR-0070 Rung 3 — declare that this host runtime owns the NMP
    /// cache-merge layer (D3-3) and is ready to receive frames with
    /// `Unchanged` projections omitted.
    ///
    /// Single-writer, set before `nmp_app_start`. Returns `Err(AlreadyStarted)`
    /// if called after start, or `Err(RegistryUnavailable)` if the registry
    /// mutex is poisoned. Returns `Ok(())` on success or on a subsequent
    /// idempotent call. In both error cases the kernel continues emitting full
    /// rows — the error is informational.
    ///
    /// S1b finding 5 (issue #1390): replaces the `debug_assert!` (silent in
    /// release) with a hard `Result` so post-start calls are caught in all
    /// build configurations.
    pub fn declare_incremental_apply(
        &self,
    ) -> Result<(), nmp_core::substrate::IncrementalApplyError> {
        use nmp_core::substrate::IncrementalApplyError;
        use std::sync::atomic::Ordering;
        if self.started.load(Ordering::SeqCst) {
            tracing::error!(
                "declare_incremental_apply called after nmp_app_start — \
                 the incremental-apply flag must be set before the kernel emits \
                 its first real frame (ADR-0070 Rung 3 / init-only invariant)"
            );
            return Err(IncrementalApplyError::AlreadyStarted);
        }
        // R6-S1: the registry's `incremental_apply_enabled` is the SINGLE source
        // of truth (an `Arc<AtomicBool>`). `declare_incremental_apply` stores
        // into it; both the kernel and any producer closure read the SAME atomic
        // (the latter via a clone from `incremental_apply_handle`). No mirror.
        self.snapshot_projections
            .lock()
            .map(|mut registry| registry.declare_incremental_apply())
            .map_err(|_| IncrementalApplyError::RegistryUnavailable)
    }

    /// ADR-0070 Rung 6 S1 — return a clone of the single-source-of-truth
    /// incremental-apply capability flag held by the `SnapshotRegistry`.
    ///
    /// The returned `Arc<AtomicBool>` is THE flag `declare_incremental_apply`
    /// sets and the kernel reads in `make_update` — not a mirror. A producer
    /// closure registered via [`NmpApp::register_typed_snapshot_projection`]
    /// captures it once at registration and reads it at tick time with
    /// `load(Acquire)`, which is the only safe way to read the capability flag
    /// from inside `run_typed()` without re-locking the `SnapshotRegistry` mutex
    /// (which would deadlock, since `run_typed` already holds it).
    ///
    /// Called once at registration time (not on the hot path), so the one-time
    /// registry lock here is irrelevant to per-tick cost. Returns a fresh
    /// false-initialised handle if the registry mutex is poisoned (D6: the
    /// producer then never omits — safe, full rows).
    #[must_use]
    pub fn incremental_apply_handle(&self) -> Arc<AtomicBool> {
        self.snapshot_projections
            .lock()
            .map(|registry| registry.incremental_apply_handle())
            .unwrap_or_else(|_| Arc::new(AtomicBool::new(false)))
    }

    /// ADR-0070 R6-S1 — return clones of the frame-identity handles
    /// `(session_id, snapshot_epoch)` the kernel publishes each tick into the
    /// shared `SnapshotRegistry`.
    ///
    /// A producer closure that omits unchanged frames captures these once at
    /// registration and reads them lock-free (`Acquire`) at tick time, forcing
    /// a rebaseline whenever EITHER value changes — the same signal the host
    /// cache resets on (`session_id` OR `snapshot_epoch`). Called once at
    /// registration (not the hot path). Returns fresh zero-handles if the
    /// registry mutex is poisoned (D6: the producer then never sees an identity
    /// change, but a poisoned registry has bigger problems; full rows are safe).
    #[must_use]
    pub fn frame_identity_handles(&self) -> (Arc<AtomicU64>, Arc<AtomicU64>) {
        self.snapshot_projections
            .lock()
            .map(|registry| registry.frame_identity_handles())
            .unwrap_or_else(|_| (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0))))
    }

    /// ADR-0070 D7 (#1671 Lane H) — a clone of the per-tick rev handle the kernel
    /// bumps at the top of every `make_update`.
    ///
    /// A [`nmp_feed::FeedWindowSource`] captures this once at registration and
    /// reads it lock-free (`Acquire`) inside both its provider and typed-producer
    /// closures, so the two share one per-tick window materialization (no
    /// `load_older` gap). Returns a fresh zero-handle if the registry mutex is
    /// poisoned (D6).
    #[must_use]
    pub fn frame_tick_rev_handle(&self) -> Arc<AtomicU64> {
        self.snapshot_projections
            .lock()
            .map(|registry| registry.frame_tick_rev_handle())
            .unwrap_or_else(|_| Arc::new(AtomicU64::new(0)))
    }

    /// ADR-0070 D7 (#1671 Lane H) — a clone of the emitted-author sink handle for
    /// the structural guardrail (BLOCKING 2).
    ///
    /// A feed's typed-producer closure captures this once at registration and
    /// writes the author keys it ENCODES onto the wire each tick (via
    /// [`nmp_core::record_emitted_feed_authors`]) WITHOUT re-locking the registry
    /// (it runs inside `run_typed()` while that mutex is held). Returns a fresh
    /// empty handle if the registry mutex is poisoned (D6).
    #[must_use]
    pub fn emitted_feed_authors_handle(&self) -> nmp_core::EmittedFeedAuthorsSlot {
        self.snapshot_projections
            .lock()
            .map(|registry| registry.emitted_feed_authors_handle())
            .unwrap_or_else(|_| std::sync::Arc::new(std::sync::Mutex::new((0, Default::default()))))
    }
}
