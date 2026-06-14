//! ADR-0055 Rung 6 Option A — trap-proof feed change-signal.
//!
//! [`FeedEmissionState`] tracks what was last emitted for the
//! `nmp.feed.home` typed projection so the producer closure can omit an
//! unchanged feed frame rather than re-sending the same ~58.8 KB payload on
//! every idle 4 Hz tick.
//!
//! ## Why this lives here (not in the engine)
//!
//! This is **emission state**, not engine state. The `RootIndexedFeed` engine
//! (`crates/nmp-feed/src/root_indexed/engine/mod.rs`) is engine state: it
//! knows about roots, attributions, pending claims, and profiles. The engine's
//! `snapshot()` + `encode_op_feed_snapshot()` path is **stateless** — it
//! re-materialises a fresh payload on every call. Emission state lives in the
//! producer closure layer (`nmp-nip01` wiring / `nmp-defaults` composition root)
//! where the host interface contract is owned.
//!
//! ## Mechanism: exact byte equality (Seam A, producer-closure omit)
//!
//! Per the ADR-0055 R6-S1 design comment on #1415:
//!
//! * Encode the snapshot to FlatBuffers bytes (`encode_op_feed_snapshot`).
//! * Compare with the last-EMITTED bytes using `==` (byte-for-byte memcmp).
//!   **No hash is used.** A hash has a nonzero collision probability; on this
//!   surface a collision = a permanently frozen feed. Exact equality is
//!   collision-proof. The buffer is ~58.8 KB — trivial to keep in memory.
//! * If identical **and** the host has declared incremental-apply capability
//!   → return `None` (omit; the host cache retains the prior value).
//! * If different (or first emit, or frame identity changed) → emit and advance
//!   the monotonic `emit_rev`.
//!
//! ## Capability gate
//!
//! Omission is gated on `incremental_apply_enabled`. When the host has NOT
//! declared incremental-apply capability, the host does NOT retain prior
//! projection values (it resets on every frame). In that case, omitting would
//! blank the feed. The guard is therefore:
//!
//! ```text
//! if !incremental_apply_enabled { always emit (byte-identical to today) }
//! ```
//!
//! ## Monotonic rev (not hash as rev)
//!
//! The host uses `incomingRev <= cached.rev` as a reorder guard. A hash is not
//! monotonic — a legitimate change whose hash value sorts below the cached rev
//! would be **dropped by the host's reorder guard** (a second, subtler freeze).
//! A monotonic counter keeps the reorder guard and the byte-equality omit
//! orthogonal and correct.
//!
//! ## Frame-identity rebaseline (the R6-S1 freeze fix)
//!
//! The host's `ProjectionCache` does `removeAll()` whenever the frame's
//! `session_id` OR `snapshot_epoch` changes (`ProjectionCache.generated.swift`).
//! The producer's emission state lives OUTSIDE the kernel, so it SURVIVES a
//! kernel rebuild (`ActorCommand::Reset`) and an account switch — the engine
//! `Arc` and `last_emitted` both persist. If the producer kept omitting after
//! the host cleared its cache, the host would have **no feed entry** → a frozen,
//! blank timeline until the next network change.
//!
//! The fix: [`should_emit`](FeedEmissionState::should_emit) takes the current
//! [`FrameIdentity`] `(session_id, snapshot_epoch)` — the EXACT tuple the kernel
//! stamps on the frame and the host resets on — and forces a full baseline
//! (treats `last_emitted` as `None`, restarts `emit_rev` from 0) whenever EITHER
//! component differs from the last-seen tuple. Producer and host therefore
//! rebaseline in **lockstep on one identical signal**, covering account-switch,
//! Reset, and any future epoch-class bump with no bespoke per-event hooks.
//!
//! ## First-emit rule
//!
//! The first emission after capability-advertise or any identity change MUST
//! always emit a full baseline. `FeedEmissionState::new()` starts with
//! `last_emitted = None` (forces the first tick to emit) and a sentinel
//! `last_identity = None` (so the very first `should_emit` is treated as an
//! identity establishment, not a spurious change).
//!
//! ## Cardinal-trap safety
//!
//! Because we compare the **exact encoded bytes the host receives**, there is no
//! bump-list to maintain and no enumeration of mutation sites. Any rendered-byte
//! change — new root, card content edit, removal, reorder, attribution
//! add/remove, profile refresh changing an author name in a visible card, or
//! window-growth — changes the encoded bytes and therefore triggers a re-emit.
//! A missed bump is structurally impossible (M1 trap-proof property; see #1415
//! comment).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The frame-level identity the host resets its projection cache on.
///
/// `session_id` = `TimingMilestones::started_unix_ms` (changes on every kernel
/// rebuild, including `ActorCommand::Reset`); `snapshot_epoch` =
/// `ProjectionRevTracker::epoch` (bumped on account-switch / schema-change).
/// The kernel publishes both each tick via `Kernel::publish_frame_identity`; the
/// producer closure reads them lock-free from the shared `Arc<AtomicU64>`
/// handles and passes them here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameIdentity {
    /// `started_unix_ms` — changes across kernel rebuilds / Reset.
    pub session_id: u64,
    /// Within-session epoch — bumped on account-switch / schema-change.
    pub snapshot_epoch: u64,
}

/// Per-producer emission state for the `nmp.feed.home` typed projection.
///
/// Persists across ticks inside the producer closure captured by
/// `register_typed_snapshot_projection`. Not shared across threads — the typed
/// projection closure runs on the actor thread under the registry's
/// `Arc<Mutex<SnapshotRegistry>>` lock, so `FeedEmissionState` does not need
/// to be `Send` itself (it is owned by the closure).
///
/// Construct with [`FeedEmissionState::new`]; call [`FeedEmissionState::should_emit`]
/// on every tick with the freshly-encoded payload and the current frame identity.
pub struct FeedEmissionState {
    /// The exact encoded bytes the host last received for `nmp.feed.home`.
    ///
    /// `None` on construction (forces the first tick to always emit) and after
    /// a frame-identity change (forces a full baseline re-emit). `Some(bytes)`
    /// holds the last-emitted FlatBuffers payload; a new payload that compares
    /// `==` to this is omitted.
    last_emitted: Option<Vec<u8>>,
    /// Monotonic per-identity rev counter. Incremented once per CHANGED
    /// emission, never on omit. Reset to 0 when `last_identity` changes so the
    /// rev sequence is scoped to the identity epoch (the host resets its cache on
    /// the same boundary, so a rev-starting-from-1 after reset is coherent).
    emit_rev: u64,
    /// The frame identity of the last emission, or `None` before the first tick.
    /// When the incoming [`FrameIdentity`] differs the state resets:
    /// `last_emitted = None`, `emit_rev = 0`, `last_identity = Some(incoming)` —
    /// guaranteeing a full baseline re-emit in lockstep with the host cache
    /// reset. See the module docs (R6-S1 freeze fix).
    last_identity: Option<FrameIdentity>,
    /// Shared flag: has the host declared incremental-apply capability?
    ///
    /// Captured once at closure-registration time as a clone of the SINGLE
    /// source of truth in the `SnapshotRegistry` (set by
    /// `declare_incremental_apply` before `nmp_app_start`; never changed after
    /// start). When `false`, the omit path is bypassed entirely — this preserves
    /// byte-identical behaviour with today for non-advertising hosts.
    incremental_apply_enabled: Arc<AtomicBool>,
}

impl FeedEmissionState {
    /// Construct a fresh emission state, not yet having emitted anything.
    ///
    /// * `incremental_apply_enabled` — the shared flag from
    ///   `AppHost::incremental_apply_handle()` (a clone of the registry's single
    ///   source of truth). When `false` at tick time, every call to
    ///   `should_emit` returns `Some(payload, rev)` (full emit, no omission) —
    ///   byte-identical to today's behavior.
    #[must_use]
    pub fn new(incremental_apply_enabled: Arc<AtomicBool>) -> Self {
        Self {
            last_emitted: None,
            emit_rev: 0,
            last_identity: None,
            incremental_apply_enabled,
        }
    }

    /// Decide whether to emit this tick.
    ///
    /// Call on every tick with the freshly-encoded FlatBuffers payload and the
    /// current [`FrameIdentity`] (read lock-free from the kernel-published
    /// `Arc<AtomicU64>` handles).
    ///
    /// Returns:
    /// - `Some((payload, rev))` — emit this payload with the given rev.
    /// - `None` — omit (host cache retains the prior value). Only returned when
    ///   `incremental_apply_enabled` is `true`, the payload is byte-identical to
    ///   the last emission, AND the frame identity is unchanged.
    ///
    /// The returned `rev` is monotonically increasing within a frame-identity
    /// epoch. The caller places it in `TypedProjectionData::projection_rev`.
    ///
    /// # Frame-identity rebaseline (freeze fix)
    ///
    /// When `identity` differs from the last-seen identity (account-switch,
    /// Reset, schema bump), the state resets and this tick ALWAYS emits a full
    /// baseline — in lockstep with the host's `ProjectionCache.removeAll()` on
    /// the same `(session_id, snapshot_epoch)` change. This guarantees the
    /// producer never omits into a freshly cleared host cache.
    ///
    /// # Capability-OFF path
    ///
    /// When `incremental_apply_enabled` is `false`, this method always returns
    /// `Some` regardless of byte equality. The state is still updated so that if
    /// the capability is later enabled (before start), the state is coherent —
    /// but in practice the flag is set-before-start and never changes after that.
    pub fn should_emit(
        &mut self,
        payload: Vec<u8>,
        identity: FrameIdentity,
    ) -> Option<(Vec<u8>, u64)> {
        let capability_on = self.incremental_apply_enabled.load(Ordering::Acquire);

        // Frame-identity change (account-switch / Reset / schema bump) — reset
        // state and force a full baseline emit, in lockstep with the host cache.
        // `None` (first tick) also takes this branch: it establishes the initial
        // identity and forces the first emit (last_emitted is already None).
        if self.last_identity != Some(identity) {
            self.last_identity = Some(identity);
            self.last_emitted = None;
            self.emit_rev = 0;
        }

        if capability_on {
            // Capability ON: check exact byte equality.
            if let Some(ref last) = self.last_emitted {
                if *last == payload {
                    // Byte-identical to last emission → omit.
                    return None;
                }
            }
            // Changed (or first emit after identity reset / construction) → emit.
            self.emit_rev += 1;
            self.last_emitted = Some(payload.clone());
            Some((payload, self.emit_rev))
        } else {
            // Capability OFF: always emit. Do NOT omit — the host does not
            // retain prior values so omitting would blank the feed.
            // We still advance the rev so it stays monotonic if the capability
            // is ever turned on before start.
            self.emit_rev += 1;
            self.last_emitted = Some(payload.clone());
            Some((payload, self.emit_rev))
        }
    }

    /// The current monotonic rev — the rev of the LAST emitted frame.
    ///
    /// `0` when nothing has been emitted yet. Used by tests to inspect state
    /// without driving a full tick.
    #[must_use]
    pub fn current_rev(&self) -> u64 {
        self.emit_rev
    }

    /// The frame identity of the last emission, or `None` before the first tick.
    #[must_use]
    pub fn current_identity(&self) -> Option<FrameIdentity> {
        self.last_identity
    }

    /// `true` if anything has been emitted since construction or last identity
    /// reset (i.e. `last_emitted` is `Some`).
    #[must_use]
    pub fn has_emitted(&self) -> bool {
        self.last_emitted.is_some()
    }
}

#[cfg(test)]
#[path = "emission_state_tests.rs"]
mod tests;
