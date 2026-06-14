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
//! * If different (or first emit, or epoch changed) → emit and advance the
//!   monotonic `emit_rev`.
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
//! This is verified by the `capability_off_always_emits` test.
//!
//! ## Monotonic rev (not hash as rev)
//!
//! The host uses `incomingRev <= cached.rev` as a reorder guard. A hash is not
//! monotonic — a legitimate change whose hash value sorts below the cached rev
//! would be **dropped by the host's reorder guard** (a second, subtler freeze).
//! A monotonic counter keeps the reorder guard and the byte-equality omit
//! orthogonal and correct.
//!
//! ## Epoch / first-emit rule
//!
//! The first emission after capability-advertise, identity reset, or epoch
//! change MUST always emit a full baseline. `FeedEmissionState::new()` starts
//! with `last_emitted = None`, which forces the first tick to always emit.
//! Epoch changes reset `last_emitted = None` so the first post-epoch tick is
//! also a forced full emit.
//!
//! ## Cardinal-trap safety
//!
//! Because we compare the **exact encoded bytes the host receives**, there is no
//! bump-list to maintain and no enumeration of mutation sites. Any rendered-byte
//! change — new root, card content edit, removal, reorder, attribution add/remove,
//! profile refresh changing an author name in a visible card, or window-growth —
//! changes the encoded bytes and therefore triggers a re-emit. A missed bump is
//! structurally impossible (M1 trap-proof property; see #1415 comment).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Per-producer emission state for the `nmp.feed.home` typed projection.
///
/// Persists across ticks inside the producer closure captured by
/// `register_typed_snapshot_projection`. Not shared across threads — the typed
/// projection closure runs on the actor thread under the registry's
/// `Arc<Mutex<SnapshotRegistry>>` lock, so `FeedEmissionState` does not need
/// to be `Send` itself (it is owned by the closure).
///
/// Construct with [`FeedEmissionState::new`]; call [`FeedEmissionState::should_emit`]
/// on every tick with the freshly-encoded payload and the current epoch.
pub struct FeedEmissionState {
    /// The exact encoded bytes the host last received for `nmp.feed.home`.
    ///
    /// `None` on construction (forces the first tick to always emit) and after
    /// an epoch change (forces a full baseline re-emit). `Some(bytes)` holds the
    /// last-emitted FlatBuffers payload; a new payload that compares `==` to
    /// this is omitted.
    last_emitted: Option<Vec<u8>>,
    /// Monotonic per-epoch rev counter. Incremented once per CHANGED emission,
    /// never on omit. Reset to 0 when `current_epoch` changes so the rev
    /// sequence is scoped to the epoch (the host resets its cache on epoch
    /// change, so a rev-starting-from-0 after reset is coherent).
    emit_rev: u64,
    /// The epoch of the last emission. When the incoming `epoch` differs from
    /// `current_epoch` the state resets: `last_emitted = None`, `emit_rev = 0`,
    /// `current_epoch = epoch`. The caller (the typed projection closure) reads
    /// the frame epoch from the `SnapshotFrame::epoch`/`session_id` signal and
    /// passes it here.
    current_epoch: u64,
    /// Shared flag: has the host declared incremental-apply capability?
    ///
    /// Captured once at closure-registration time as an `Arc<AtomicBool>` (set
    /// by `declare_incremental_apply` before `nmp_app_start`; never changed
    /// after start). When `false`, the omit path is bypassed entirely — this
    /// preserves byte-identical behaviour with today for non-advertising hosts.
    incremental_apply_enabled: Arc<AtomicBool>,
}

impl FeedEmissionState {
    /// Construct a fresh emission state, not yet having emitted anything.
    ///
    /// * `incremental_apply_enabled` — the shared flag from
    ///   `NmpApp::incremental_apply_handle()`. When `false` at tick time, every
    ///   call to `should_emit` returns `Some(payload, rev)` (full emit, no
    ///   omission) — byte-identical to today's behavior.
    #[must_use]
    pub fn new(incremental_apply_enabled: Arc<AtomicBool>) -> Self {
        Self {
            last_emitted: None,
            emit_rev: 0,
            current_epoch: 0,
            incremental_apply_enabled,
        }
    }

    /// Decide whether to emit this tick.
    ///
    /// Call on every tick with the freshly-encoded FlatBuffers payload and the
    /// current frame epoch (from the snapshot frame's epoch / session-id signal).
    ///
    /// Returns:
    /// - `Some((payload, rev))` — emit this payload with the given rev.
    /// - `None` — omit (host cache retains the prior value). Only returned when
    ///   `incremental_apply_enabled` is `true`, the payload is byte-identical to
    ///   the last emission, and the epoch is unchanged.
    ///
    /// The returned `rev` is monotonically increasing within an epoch. The
    /// caller places it in `TypedProjectionData::projection_rev`.
    ///
    /// # Capability-OFF path
    ///
    /// When `incremental_apply_enabled` is `false`, this method always returns
    /// `Some` regardless of byte equality. The returned payload and rev are still
    /// updated internally so that if the capability is later enabled (before
    /// start), the state is coherent — but in practice the flag is set-before-
    /// start and never changes after that.
    pub fn should_emit(&mut self, payload: Vec<u8>, epoch: u64) -> Option<(Vec<u8>, u64)> {
        let capability_on = self.incremental_apply_enabled.load(Ordering::Acquire);

        // Epoch change — reset state and force a full baseline emit.
        if epoch != self.current_epoch {
            self.current_epoch = epoch;
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
            // Changed (or first emit after epoch reset / construction) → emit.
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

    /// The current epoch tracked by this state.
    #[must_use]
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// `true` if anything has been emitted since construction or last epoch
    /// reset (i.e. `last_emitted` is `Some`).
    #[must_use]
    pub fn has_emitted(&self) -> bool {
        self.last_emitted.is_some()
    }
}

#[cfg(test)]
#[path = "emission_state_tests.rs"]
mod tests;
