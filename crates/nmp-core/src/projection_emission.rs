//! ADR-0070 Rung 6 — reusable per-typed-projection omit helper.
//!
//! [`TypedProjectionEmissionState`] is the trap-proof byte-equality omit
//! mechanism first built for an app-owned feed session (R6-S1) and generalised
//! here (R6-S2) so ANY typed projection producer can use the same omit logic
//! without duplication.
//!
//! ## Mechanism
//!
//! * Encode the projection to its FlatBuffers payload bytes.
//! * Compare with the last-EMITTED bytes using `==` (byte-for-byte memcmp).
//!   **No hash is used.** A hash collision = a permanently frozen projection.
//!   Exact equality is collision-proof.
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
//! blank the projection. The guard:
//!
//! ```text
//! if !incremental_apply_enabled { always emit (byte-identical to today) }
//! ```
//!
//! ## Frame-identity rebaseline (the R6-S1 freeze fix, carried into S2)
//!
//! The host's `ProjectionCache` does `removeAll()` whenever the frame's
//! `session_id` OR `snapshot_epoch` changes. A producer's emission state lives
//! OUTSIDE the kernel, so it SURVIVES a kernel rebuild (`ActorCommand::Lifecycle(LifecycleCommand::Reset)`)
//! and an account switch. If the producer kept omitting after the host cleared
//! its cache, the host would have NO projection entry → a frozen, blank UI.
//!
//! Fix: [`should_emit`](TypedProjectionEmissionState::should_emit) takes the
//! current [`FrameIdentity`] `(session_id, snapshot_epoch)` and forces a full
//! baseline (treats `last_emitted` as `None`, restarts `emit_rev` from 0)
//! whenever EITHER component differs from the last-seen tuple. Producer and host
//! therefore rebaseline in **lockstep on one identical signal**, covering
//! account-switch, Reset, and any future epoch-class bump.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The frame-level identity the host resets its projection cache on.
///
/// `session_id` = `TimingMilestones::started_unix_ms` (changes on every kernel
/// rebuild, including `ActorCommand::Lifecycle(LifecycleCommand::Reset)`); `snapshot_epoch` =
/// `ProjectionRevTracker::epoch` (bumped on account-switch / schema-change).
/// The kernel publishes both each tick via `Kernel::publish_frame_identity`; the
/// producer closure reads them lock-free from shared `Arc<AtomicU64>` handles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameIdentity {
    /// `started_unix_ms` — changes across kernel rebuilds / Reset.
    pub session_id: u64,
    /// Within-session epoch — bumped on account-switch / schema-change.
    pub snapshot_epoch: u64,
}

/// Per-producer emission state for any typed projection.
///
/// Persists across ticks inside the producer closure captured by
/// `register_typed_snapshot_projection`. Not shared across threads — wrap in
/// `Arc<Mutex<…>>` when the closure must be `Send + Sync` (the production
/// pattern; the lock is uncontested).
///
/// Construct with [`TypedProjectionEmissionState::new`]; call
/// [`TypedProjectionEmissionState::should_emit`] on every tick with the
/// freshly-encoded payload and the current frame identity.
pub struct TypedProjectionEmissionState {
    /// The exact encoded bytes the host last received.
    ///
    /// `None` on construction (forces the first tick to always emit) and after
    /// a frame-identity change (forces a full baseline re-emit). `Some(bytes)`
    /// holds the last-emitted payload; a new payload that compares `==` to this
    /// is omitted.
    last_emitted: Option<Vec<u8>>,
    /// Monotonic per-identity rev counter. Incremented once per CHANGED
    /// emission, never on omit. Reset to 0 when `last_identity` changes so the
    /// rev sequence is scoped to the identity epoch (the host resets its cache
    /// on the same boundary).
    emit_rev: u64,
    /// The frame identity of the last emission, or `None` before the first tick.
    /// When the incoming [`FrameIdentity`] differs the state resets:
    /// `last_emitted = None`, `emit_rev = 0`, `last_identity = Some(incoming)` —
    /// guaranteeing a full baseline re-emit in lockstep with the host cache
    /// reset.
    last_identity: Option<FrameIdentity>,
    /// Shared flag: has the host declared incremental-apply capability?
    ///
    /// When `false`, the omit path is bypassed entirely — preserves byte-identical
    /// behaviour with today for non-advertising hosts.
    incremental_apply_enabled: Arc<AtomicBool>,
}

impl TypedProjectionEmissionState {
    /// Construct a fresh emission state, not yet having emitted anything.
    ///
    /// * `incremental_apply_enabled` — the shared flag from
    ///   `AppHost::incremental_apply_handle()`. When `false` at tick time,
    ///   every call to `should_emit` returns `Some(payload, rev)` — byte-identical
    ///   to today's behavior.
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
    /// current [`FrameIdentity`].
    ///
    /// Returns:
    /// - `Some((payload, rev))` — emit this payload with the given rev.
    /// - `None` — omit (host cache retains the prior value). Only returned when
    ///   `incremental_apply_enabled` is `true`, the payload is byte-identical to
    ///   the last emission, AND the frame identity is unchanged.
    ///
    /// The returned `rev` is monotonically increasing within a frame-identity
    /// epoch.
    ///
    /// # Frame-identity rebaseline (freeze fix)
    ///
    /// When `identity` differs from the last-seen identity, the state resets and
    /// this tick ALWAYS emits a full baseline — in lockstep with the host's
    /// `ProjectionCache.removeAll()` on the same `(session_id, snapshot_epoch)`
    /// change.
    ///
    /// # Capability-OFF path
    ///
    /// When `incremental_apply_enabled` is `false`, this method always returns
    /// `Some` regardless of byte equality.
    pub fn should_emit(
        &mut self,
        payload: Vec<u8>,
        identity: FrameIdentity,
    ) -> Option<(Vec<u8>, u64)> {
        let capability_on = self.incremental_apply_enabled.load(Ordering::Acquire);

        // Frame-identity change — reset state and force a full baseline emit.
        // `None` (first tick) also takes this branch.
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
            // Changed (or first emit after identity reset) → emit.
            self.emit_rev += 1;
            self.last_emitted = Some(payload.clone());
            Some((payload, self.emit_rev))
        } else {
            // Capability OFF: always emit. Do NOT omit.
            self.emit_rev += 1;
            self.last_emitted = Some(payload.clone());
            Some((payload, self.emit_rev))
        }
    }

    /// The current monotonic rev — the rev of the LAST emitted frame.
    ///
    /// `0` when nothing has been emitted yet.
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
#[path = "projection_emission_tests.rs"]
mod projection_emission_tests;
