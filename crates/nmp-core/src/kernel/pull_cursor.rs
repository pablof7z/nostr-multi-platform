//! Pull-cursor registry — ADR-0058 §10, step 3a.
//!
//! Non-durable registry of pull cursors.  Single writer: the actor thread via
//! `OpenPullCursor` / `AdvancePullCursor` / `UnregisterPullCursor` dispatch arms.
//! Shared behind `Arc<RwLock<…>>` so the FFI `pull_page` read path can snapshot
//! a registration without an actor round-trip.
//!
//! ## Cursor-id allocation — hosts never mint raw ids
//!
//! [`PullCursorRegistry::alloc_handle`] is the single allocation point. The FFI
//! layer calls it under a brief write lock before dispatching
//! [`crate::actor::ActorCommand::OpenPullCursor`]; the actor validates and stores
//! the row; the host stores the returned [`PullCursorHandle`].
//!
//! ## Wake interplay
//!
//! Register and advance arm an immediate wake whenever `after_seq <
//! latest_ingest_seq`. Unregister removes the row and any pending wake entry.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use super::pull::{PullLimits, PullScope};
use super::Kernel;

/// Hard ceiling on simultaneously-registered pull cursors (D5: bounded).
/// Registrations past the cap (for a *new* `cursor_id`) are loud no-ops.
pub const MAX_PULL_CURSORS: usize = 128;

// ─── Identifier types ────────────────────────────────────────────────────────

/// Internal cursor id.  `0` is reserved/invalid (never armed, never stored).
///
/// Allocated only by [`PullCursorRegistry::alloc_handle`] — external code
/// should hold a [`PullCursorHandle`] instead of constructing this directly.
/// The inner `u64` remains `pub` only for the FlatBuffers wire codec and FFI
/// read paths that must serialise the id.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PullCursorId(pub u64);

/// Opaque handle returned to the caller by
/// [`PullCursorRegistry::alloc_handle`].
///
/// Hosts store this value and pass it to `AdvancePullCursor` /
/// `UnregisterPullCursor`.  The inner [`PullCursorId`] is accessible only via
/// [`id()`](PullCursorHandle::id) to prevent accidental raw-integer casting at
/// call sites.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PullCursorHandle(PullCursorId);

impl PullCursorHandle {
    /// The underlying registry id — for use by the actor dispatch seam and the
    /// FFI `pull_page` reader.
    #[must_use]
    pub fn id(self) -> PullCursorId {
        self.0
    }

    /// Construct a handle from a raw id — available only in test / test-support
    /// builds. Production code must use
    /// [`PullCursorRegistry::alloc_handle`] instead.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn from_raw(id: u64) -> Self {
        PullCursorHandle(PullCursorId(id))
    }
}

/// Typed consumer identity — replaces the raw `String` that previously appeared
/// in [`PullCursorRegistration`] and `ActorCommand::OpenPullCursor`.
///
/// The kernel treats the value as an opaque tag; it is carried through the
/// registry for step-4 (retention claims) and diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullConsumerId(pub String);

impl std::fmt::Display for PullConsumerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for PullConsumerId {
    fn from(s: &str) -> Self {
        PullConsumerId(s.to_owned())
    }
}

impl From<String> for PullConsumerId {
    fn from(s: String) -> Self {
        PullConsumerId(s)
    }
}

// ─── Validation error ────────────────────────────────────────────────────────

/// Error returned when a [`PullCursorSpec`] is structurally invalid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvalidCursorSpec {
    /// `limits.max_entries` exceeds `limits.max_scan_entries`.
    ///
    /// Every page would be capped by `max_entries` before `max_scan_entries`
    /// rows are visited, making the scan budget unreachable.
    LimitsOutOfOrder {
        max_entries: usize,
        max_scan_entries: usize,
    },
}

impl std::fmt::Display for InvalidCursorSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvalidCursorSpec::LimitsOutOfOrder { max_entries, max_scan_entries } => write!(
                f,
                "PullCursorSpec: max_entries ({max_entries}) > max_scan_entries \
                 ({max_scan_entries}); max_entries must be ≤ max_scan_entries"
            ),
        }
    }
}

// ─── PullCursorSpec ──────────────────────────────────────────────────────────

/// Everything the host provides when opening a new pull cursor — **without** a
/// cursor id.
///
/// Pass this to [`PullCursorRegistry::alloc_handle`] (via the FFI entry point)
/// to obtain an opaque [`PullCursorHandle`]; then dispatch
/// [`crate::actor::ActorCommand::OpenPullCursor`] carrying both.  The kernel
/// validates the id (0 is rejected) and stores the registration.
#[derive(Clone, Debug)]
pub struct PullCursorSpec {
    pub consumer_id: PullConsumerId,
    pub scope: PullScope,
    pub mode: PullCursorMode,
    /// Start the cursor at this sequence position (exclusive).
    /// `0` starts from the beginning of the log.
    pub after_seq: u64,
    pub limits: PullLimits,
}

impl PullCursorSpec {
    /// Validate the spec's limits, returning `Err` when they are contradictory.
    ///
    /// Currently enforces `max_entries ≤ max_scan_entries`.
    ///
    /// # Errors
    /// Returns [`InvalidCursorSpec::LimitsOutOfOrder`] when
    /// `max_entries > max_scan_entries`.
    pub fn validate(&self) -> Result<(), InvalidCursorSpec> {
        let me = self.limits.max_entries.get();
        let ms = self.limits.max_scan_entries.get();
        if me > ms {
            return Err(InvalidCursorSpec::LimitsOutOfOrder {
                max_entries: me,
                max_scan_entries: ms,
            });
        }
        Ok(())
    }
}

// ─── PullCursorMode ──────────────────────────────────────────────────────────

/// Retention disposition for a registered cursor (ADR §6). The advanced
/// `Protected` floor-pin behavior lands in step-4; step-3a only carries the
/// declared mode through the registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PullCursorMode {
    /// The log GC may prune past this cursor; falling behind yields an explicit
    /// `PullGap` rather than a silent skip.
    GapAllowed,
    /// The log GC may not prune past this cursor until its lag exceeds
    /// `max_lag_entries`, after which the claim is dropped (step-4).
    Protected { max_lag_entries: u64 },
}

// ─── PullCursorRegistration ──────────────────────────────────────────────────

/// One registered cursor row. Cloned out under the registry read-lock by the
/// (step-3b) FFI `pull_page` path; written only on the actor thread.
///
/// `scope` / `after_seq` / `limits` are read by the FFI `pull_page` snapshot
/// seam; `consumer_id` / `mode` are carried for step-4 (retention claims) and
/// diagnostics — written here but not yet read, so `allow(dead_code)` documents
/// the forward-looking fields rather than masking genuine dead code.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PullCursorRegistration {
    pub cursor_id: PullCursorId,
    pub consumer_id: PullConsumerId,
    pub scope: PullScope,
    pub mode: PullCursorMode,
    pub after_seq: u64,
    pub limits: PullLimits,
}

// ─── PullCursorRegistry ──────────────────────────────────────────────────────

/// Actor-written, FFI-read-only registry of pull cursors.
/// Owns the monotonic cursor-id counter; [`alloc_handle`](Self::alloc_handle)
/// is the only allocation point.
pub struct PullCursorRegistry {
    by_id: BTreeMap<PullCursorId, PullCursorRegistration>,
    /// Monotonic counter. Starts at 1; 0 is the sentinel "invalid".
    next_cursor_id: u64,
}

impl Default for PullCursorRegistry {
    fn default() -> Self {
        Self { by_id: BTreeMap::new(), next_cursor_id: 1 }
    }
}

/// Shared handle to the registry. Single writer (actor) for registrations;
/// brief write lock held by FFI for [`alloc_handle`](PullCursorRegistry::alloc_handle).
pub type PullCursorRegistrySlot = Arc<RwLock<PullCursorRegistry>>;

impl PullCursorRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate a fresh [`PullCursorHandle`] (the only id-minting point).
    /// Call this under a brief write lock before dispatching
    /// [`crate::actor::ActorCommand::OpenPullCursor`].
    #[must_use]
    pub fn alloc_handle(&mut self) -> PullCursorHandle {
        let id = self.next_cursor_id;
        // wrapping_add + max(1) keeps the counter non-zero after u64::MAX.
        self.next_cursor_id = self.next_cursor_id.wrapping_add(1).max(1);
        PullCursorHandle(PullCursorId(id))
    }

    /// Number of currently-registered cursors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the registry holds no cursors.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Clone a registration by id (FFI `pull_page` snapshot seam; used by tests).
    #[must_use]
    pub fn get(&self, id: &PullCursorId) -> Option<PullCursorRegistration> {
        self.by_id.get(id).cloned()
    }

    /// Iterate `(cursor_id, after_seq)` pairs — the only fields the wake arm
    /// needs. Borrows the registry read-side.
    fn iter_seqs(&self) -> impl Iterator<Item = (PullCursorId, u64)> + '_ {
        self.by_id.values().map(|r| (r.cursor_id, r.after_seq))
    }

    /// Build the `Protected`-cursor log-retention claim set (ADR-0058 §6,
    /// step-4). `GapAllowed` cursors publish nothing; each `Protected` cursor
    /// publishes `(after_seq, max_lag_entries)`. The kernel forwards this to
    /// `EventStore::replace_log_retention_claims` after every registry mutation
    /// (it is the single writer of the claim set).
    fn retention_claims(&self) -> Vec<crate::store::LogRetentionClaim> {
        self.by_id
            .values()
            .filter_map(|r| match r.mode {
                PullCursorMode::Protected { max_lag_entries } => {
                    Some(crate::store::LogRetentionClaim {
                        after_seq: r.after_seq,
                        max_lag_entries,
                    })
                }
                PullCursorMode::GapAllowed => None,
            })
            .collect()
    }
}

// ─── Kernel methods ──────────────────────────────────────────────────────────

// The three command methods + their wake helper are live via the actor
// dispatch loop in default/native builds; that loop is feature-gated out of the
// `--no-default-features` (wasm) build, where they read as dead. They are also
// exercised by `pull_cursor_wake_tests`. `allow(dead_code)` documents the
// feature-conditional liveness rather than masking genuine dead code.
#[allow(dead_code)]
impl Kernel {
    /// Reconcile this cursor's pull wake against the store head.
    ///
    /// Coalesced: the wake map holds at most one entry per cursor, set to the
    /// latest observed `latest_ingest_seq` (max of any prior value). When the
    /// cursor has caught up (`after_seq >= latest`) any **existing** pending
    /// wake is REMOVED — otherwise an advance-to-head would leave a stale entry
    /// that the next `drain_pull_wakes` emits as a duplicate (no-double-count
    /// contract). A store read error is non-fatal — the map is left unchanged.
    fn update_pull_wake(&mut self, cursor_id: PullCursorId, after_seq: u64) {
        let latest = match self.store.latest_ingest_seq() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "latest_ingest_seq failed; pull wake not reconciled");
                return;
            }
        };
        if after_seq < latest {
            let entry = self.store_wakeups.pull.entry(cursor_id).or_insert(0);
            *entry = (*entry).max(latest);
        } else {
            // Caught up — clear any stale pending wake so it is not re-emitted.
            self.store_wakeups.pull.remove(&cursor_id);
        }
    }

    /// Rebuild the `Protected`-cursor retention claims from the registry and
    /// publish them to the store (ADR-0058 §6, step-4).
    ///
    /// The kernel is the single writer of the claim set: this is called after
    /// EVERY register / advance / unregister so the store's append-time log trim
    /// always pins to the current slowest still-eligible protected cursor.
    fn publish_retention_claims(&self) {
        let registry = Arc::clone(&self.pull_cursor_registry);
        let claims = {
            let reg = registry.read().expect("pull cursor registry poisoned");
            reg.retention_claims()
        };
        self.store.replace_log_retention_claims(&claims);
    }

    /// Open (or replace) a pull cursor — `ActorCommand::OpenPullCursor`.
    ///
    /// The caller obtains `handle` from
    /// [`PullCursorRegistry::alloc_handle`] before dispatching the command;
    /// the kernel validates it here (id 0 is invalid) and stores the row.
    ///
    /// Replace-by-`cursor_id` is always allowed (it does not grow the set). A
    /// *new* registration past `MAX_PULL_CURSORS` is a loud no-op (log, never
    /// panic). Arms an immediate wake when `after_seq < latest_ingest_seq`.
    pub(crate) fn open_pull_cursor(&mut self, handle: PullCursorHandle, spec: PullCursorSpec) {
        let cursor_id = handle.id();
        if cursor_id.0 == 0 {
            tracing::warn!("OpenPullCursor ignored: cursor_id 0 is invalid");
            return;
        }
        let after_seq = spec.after_seq;
        {
            let registry = Arc::clone(&self.pull_cursor_registry);
            let mut reg = registry.write().expect("pull cursor registry poisoned");
            let is_replace = reg.by_id.contains_key(&cursor_id);
            if !is_replace && reg.len() >= MAX_PULL_CURSORS {
                tracing::warn!(
                    cursor_id = cursor_id.0,
                    max = MAX_PULL_CURSORS,
                    "OpenPullCursor ignored: MAX_PULL_CURSORS reached (loud no-op)"
                );
                return;
            }
            reg.by_id.insert(
                cursor_id,
                PullCursorRegistration {
                    cursor_id,
                    consumer_id: spec.consumer_id,
                    scope: spec.scope,
                    mode: spec.mode,
                    after_seq,
                    limits: spec.limits,
                },
            );
        }
        self.update_pull_wake(cursor_id, after_seq);
        // ADR-0058 §6 step-4: republish the protected-cursor retention claims.
        self.publish_retention_claims();
    }

    /// Monotonically advance a cursor — `ActorCommand::AdvancePullCursor`.
    ///
    /// `after_seq = max(old, new)`. An unknown cursor id is a silent no-op
    /// (the consumer may have unregistered concurrently). Re-arms an immediate
    /// wake when the cursor is still behind the store head.
    pub(crate) fn advance_pull_cursor(&mut self, cursor_id: PullCursorId, after_seq: u64) {
        let new_after = {
            let registry = Arc::clone(&self.pull_cursor_registry);
            let mut reg = registry.write().expect("pull cursor registry poisoned");
            let Some(row) = reg.by_id.get_mut(&cursor_id) else {
                return;
            };
            row.after_seq = row.after_seq.max(after_seq);
            row.after_seq
        };
        self.update_pull_wake(cursor_id, new_after);
        // ADR-0058 §6 step-4: an advanced protected cursor moves its claim's
        // after_seq forward — republish so the log floor can follow it.
        self.publish_retention_claims();
    }

    /// Unregister a cursor — `ActorCommand::UnregisterPullCursor`.
    ///
    /// Removes the registry row **and** any pending `StoreWakeups.pull` entry so
    /// a withdrawn consumer never re-fires a wake.
    pub(crate) fn unregister_pull_cursor(&mut self, cursor_id: PullCursorId) {
        {
            let registry = Arc::clone(&self.pull_cursor_registry);
            registry
                .write()
                .expect("pull cursor registry poisoned")
                .by_id
                .remove(&cursor_id);
        }
        self.store_wakeups.pull.remove(&cursor_id);
        // ADR-0058 §6 step-4: a withdrawn protected cursor drops its claim.
        self.publish_retention_claims();
    }

    /// Re-arm every registered cursor still behind the store head — the
    /// level-triggered half of the drain contract (called from
    /// [`Kernel::drain_pull_wakes`]). Reinserts into the **same**
    /// `StoreWakeups.pull` map; it is not a second wake path.
    pub(in crate::kernel) fn rearm_pull_wakes_still_behind(&mut self) {
        let latest = match self.store.latest_ingest_seq() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "latest_ingest_seq failed; pull re-arm skipped");
                return;
            }
        };
        let registry = Arc::clone(&self.pull_cursor_registry);
        let behind: Vec<PullCursorId> = {
            let reg = registry.read().expect("pull cursor registry poisoned");
            reg.iter_seqs()
                .filter(|(_, after_seq)| *after_seq < latest)
                .map(|(id, _)| id)
                .collect()
        };
        for id in behind {
            let entry = self.store_wakeups.pull.entry(id).or_insert(0);
            *entry = (*entry).max(latest);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::*;
    use crate::kernel::pull::{PullLimits, PullScope};

    fn limits(max_entries: usize, max_scan: usize) -> PullLimits {
        PullLimits {
            max_entries: NonZeroUsize::new(max_entries).unwrap(),
            max_scan_entries: NonZeroUsize::new(max_scan).unwrap(),
        }
    }

    fn spec(max_entries: usize, max_scan: usize) -> PullCursorSpec {
        PullCursorSpec {
            consumer_id: PullConsumerId("test".into()),
            scope: PullScope::GlobalLog,
            mode: PullCursorMode::GapAllowed,
            after_seq: 0,
            limits: limits(max_entries, max_scan),
        }
    }

    #[test]
    fn validate_ok_when_entries_le_scan() {
        assert!(spec(64, 256).validate().is_ok());
        assert!(spec(256, 256).validate().is_ok(), "equal is valid");
    }

    #[test]
    fn validate_err_when_entries_gt_scan() {
        let err = spec(257, 256).validate().unwrap_err();
        assert_eq!(
            err,
            InvalidCursorSpec::LimitsOutOfOrder { max_entries: 257, max_scan_entries: 256 }
        );
    }

    #[test]
    fn alloc_handle_yields_sequential_nonzero_ids() {
        let mut reg = PullCursorRegistry::new();
        let h1 = reg.alloc_handle();
        let h2 = reg.alloc_handle();
        assert_ne!(h1, h2, "each alloc yields a distinct handle");
        assert_ne!(h1.id().0, 0, "id 0 is never allocated");
        assert_ne!(h2.id().0, 0);
        assert!(h2.id().0 > h1.id().0, "ids are strictly increasing");
    }

    #[test]
    fn pull_consumer_id_display_and_from() {
        let id = PullConsumerId::from("mirror");
        assert_eq!(id.to_string(), "mirror");
        let id2: PullConsumerId = "feed".to_string().into();
        assert_eq!(id2.0, "feed");
    }
}
