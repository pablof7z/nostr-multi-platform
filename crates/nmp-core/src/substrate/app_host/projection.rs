//! Snapshot / typed / tick projection registration and the incremental
//! emission contract (ADR-0037 / ADR-0055).
//!
//! Split out of `app_host/mod.rs` (D6 work) to keep that file under the 500-LOC
//! hard ceiling — this is the single largest narrow registration concern.

use crate::update_envelope::TypedProjectionData;

/// Error returned by [`SnapshotProjectionRegistrar::declare_incremental_apply`]
/// when the pre-start invariant is violated or the registry is unavailable.
///
/// ADR-0055 Rung 3 S1b (finding 5 / issue #1390) — replaces the
/// `debug_assert!` (silent in release) with a hard `Result` return-code so
/// a post-start call is caught in all builds, not only debug.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IncrementalApplyError {
    /// `declare_incremental_apply` was called AFTER `nmp_app_start`.
    /// The incremental-apply flag must be set before the kernel emits its
    /// first real frame (ADR-0055 Rung 3 / init-only invariant).
    AlreadyStarted,
    /// The snapshot-projection registry mutex was poisoned (another thread
    /// panicked while holding it). Treat as a non-recoverable kernel error.
    RegistryUnavailable,
}

/// Register snapshot / typed / tick projections and configure the incremental
/// emission contract (ADR-0037 / ADR-0055).
///
/// The projection-registration concern: snapshot data closures, typed
/// FlatBuffers sidecars, per-tick observers, the consumed-projection
/// declaration, and the incremental-apply / frame-identity handles a producer
/// captures to keep its omit-memory in lockstep with the host cache.
pub trait SnapshotProjectionRegistrar {
    /// Register a **typed** FlatBuffers projection closure under `key` — the
    /// typed-sidecar counterpart to [`Self::register_snapshot_projection`]
    /// (ADR-0037). The closure returns the projection's opaque, host-declared
    /// FlatBuffers payload ([`TypedProjectionData`]) carried verbatim in the
    /// `SnapshotFrame`'s `typed_projections` sidecar, or `None` when there is
    /// no changed row to emit this tick. Under incremental apply, omission
    /// means retain the last decoded value; removal of the registered key emits
    /// an explicit `Cleared` row.
    ///
    /// This method lives on the trait — not only on the concrete `NmpApp` — so
    /// reusable protocol/feed crates that register through `&impl
    /// SnapshotProjectionRegistrar` (e.g. `register_runtime`) can wire typed
    /// projections without depending on the C-ABI crate. It mirrors
    /// `register_snapshot_projection`: `&self` (the registry mutation is a
    /// lock-and-insert), and the same host-chosen key space shared with the
    /// generic registry (ADR-0037 Commitment 4).
    ///
    /// Like the generic closure, `f` runs on the actor thread inside the
    /// snapshot tick — it MUST be non-blocking (D8).
    fn register_typed_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> Option<TypedProjectionData> + Send + Sync + 'static;

    /// Register a **per-tick observer** — a no-result callback fired once on
    /// every snapshot tick, the generic projection-free counterpart to
    /// [`Self::register_snapshot_projection`].
    ///
    /// Where a projection closure produces snapshot *data* under a key, a tick
    /// observer produces nothing: it is a pure per-tick side-effect seam for
    /// host-side reconcilers that need a "the kernel just ticked" callback but
    /// contribute no projection output. The canonical consumer is an
    /// active-account subscription reconciler that diffs the active pubkey each
    /// tick and enqueues `PushInterest` / `WithdrawInterest` actor commands —
    /// previously such reconcilers abused the projection registry by returning a
    /// `Value::Null` projection purely to obtain the per-tick callback.
    ///
    /// This method lives on the trait — not only on the concrete `NmpApp` — so
    /// reusable protocol/runtime crates that register through `&impl
    /// SnapshotProjectionRegistrar` (e.g. `register_zap_receipts_runtime`) can
    /// wire a per-tick reconciler without depending on the C-ABI crate. It
    /// mirrors `register_snapshot_projection`: `&self` (the registry mutation is
    /// a lock-and-push), and the same shared registry/slot.
    ///
    /// Like a projection closure, `f` runs on the actor thread inside the
    /// snapshot tick — it MUST be non-blocking (D8: enqueue only, no I/O or
    /// lock waits). A panicking observer is contained (D6) and cannot crash the
    /// tick.
    fn register_snapshot_tick_observer<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static;

    /// ADR-0055 Rung 3 — declare that this host runtime owns the NMP
    /// cache-merge layer (D3-3) and is ready to receive frames with
    /// `Unchanged` projections omitted.
    ///
    /// Single-writer, set before `nmp_app_start`. After this call the kernel
    /// guarantees the NEXT `make_update` frame is a full baseline (all live
    /// Tier-2 projections emitted as `Changed`). Until this is called the
    /// kernel emits full rows on every tick (no behavior change for
    /// non-advertising hosts). Idempotent — subsequent calls before start
    /// return `Ok(())` without re-setting the latch.
    ///
    /// Returns `Err(AlreadyStarted)` when called after `nmp_app_start` (the
    /// incremental-apply flag must be set before the kernel's first real
    /// frame), or `Err(RegistryUnavailable)` when the registry mutex is
    /// poisoned. In both error cases the kernel continues emitting full rows —
    /// the error is informational, not fatal to the kernel.
    ///
    /// This is durable architecture (the per-attach baseline gate + the
    /// Rung-5 ADR-0053 compose seam), NOT a compat shim.
    fn declare_incremental_apply(&self) -> Result<(), IncrementalApplyError>;

    /// ADR-0055 Rung 6 S1 — return a shared handle to the incremental-apply
    /// capability flag.
    ///
    /// The returned `Arc<std::sync::atomic::AtomicBool>` is `true` when
    /// [`Self::declare_incremental_apply`] has been called, `false` otherwise.
    /// Producer closures registered via
    /// [`Self::register_typed_snapshot_projection`] capture this handle and
    /// read it at tick time with `load(Acquire)` — the `Arc<AtomicBool>` is the
    /// only safe way to read the capability flag from inside `run_typed()` without
    /// re-locking the `SnapshotRegistry` mutex (which is already held by
    /// `run_typed` and cannot be re-entered without deadlock).
    ///
    /// R6-S1: this is a clone of the SINGLE-source-of-truth flag in the
    /// `SnapshotRegistry` — the same atomic `declare_incremental_apply` sets and
    /// the kernel reads. There is no separate mirror.
    fn incremental_apply_handle(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool>;

    /// ADR-0055 R6-S1 — return clones of the frame-identity handles
    /// `(session_id, snapshot_epoch)` the kernel publishes each tick.
    ///
    /// A Tier-1 producer closure that omits unchanged frames (the feed
    /// change-signal) captures these once at registration and reads them
    /// lock-free (`Acquire`) at tick time, forcing a full-baseline re-emit
    /// whenever EITHER value changes — the SAME signal the host's
    /// `ProjectionCache` resets on. This keeps the producer's omit memory and
    /// the host cache in lockstep across account-switch AND `Reset` AND any
    /// future epoch-class event, so the producer can never omit into a freshly
    /// cleared host cache (the R6-S1 freeze fix).
    ///
    /// `session_id` = `TimingMilestones::started_unix_ms` (changes on every
    /// kernel rebuild including `ActorCommand::Lifecycle(LifecycleCommand::Reset)`); `snapshot_epoch` =
    /// `ProjectionRevTracker::epoch` (account-switch / schema bump).
    fn frame_identity_handles(
        &self,
    ) -> (
        std::sync::Arc<std::sync::atomic::AtomicU64>,
        std::sync::Arc<std::sync::atomic::AtomicU64>,
    );

    /// Remove the typed snapshot projection registered under `key`, if any.
    ///
    /// Used by lifecycle-managed projection sessions (e.g. group-discovery
    /// teardown in NIP-29) to clear a key so no stale row is emitted after the
    /// session ends. Idempotent — an unknown key is a silent no-op. A poisoned
    /// registry lock is a silent no-op (D6).
    fn remove_snapshot_projection(&self, key: &str);

    /// ADR-0053 — declare the static set of **Tier-2 built-in projection keys**
    /// this host consumes (the union of every projection any of the app's screens
    /// can read, known at app build time).
    ///
    /// The output-side sibling of the relay `push_interest` lattice: the kernel
    /// serializes a kernel-owned built-in into each snapshot only if its key is
    /// in the declared set. An **empty** declared set means "no opinion" and
    /// emits every built-in (no narrowing — the relay-filter semantic, where an
    /// empty filter set does not subscribe to nothing). A **non-empty** set
    /// narrows the built-ins to its members, skipping the producer work (no
    /// serialize, no roll-up) for everything else — most notably the
    /// `relay_diagnostics` roll-up, which no longer ships to hosts that do not
    /// declare it.
    ///
    /// Additive (unions into the set) and `&self` (the mutation is a
    /// lock-and-extend behind the shared registry slot). Intended as a host-init
    /// call, before `nmp_app_start`. Tier-1 host/protocol projections registered
    /// via [`Self::register_snapshot_projection`] are NOT gated by this —
    /// registration already declares their consumption (and dynamic feeds gate by
    /// their `unregister_feed` lifecycle).
    fn declare_consumed_projections<I, K>(&self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>;
}
