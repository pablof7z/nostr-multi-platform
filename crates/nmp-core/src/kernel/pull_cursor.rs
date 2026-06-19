//! Pull-cursor registry — ADR-0058 §10, step 3a.
//!
//! A non-durable, actor-written registry of registered pull cursors. The
//! consumer-persisted `after_seq` is the durable source of truth; this registry
//! is rebuilt by the host re-registering its cursors after a restart.
//!
//! ## Ownership / threading
//!
//! The registry lives behind a [`PullCursorRegistrySlot`] (`Arc<RwLock<…>>`) so
//! a future read-only FFI `pull_page` (step 3b) can snapshot a registration on
//! another thread without an actor round-trip. In step 3a the **only** writer is
//! the actor thread via the three fire-and-forget [`crate::actor::ActorCommand`]
//! dispatch arms (`RegisterPullCursor` / `AdvancePullCursor` /
//! `UnregisterPullCursor`).
//!
//! ## Wake interplay (the level-triggered contract)
//!
//! Register and advance both **arm an immediate wake** (an entry in
//! `StoreWakeups.pull`) whenever the cursor's `after_seq` is behind
//! `latest_ingest_seq` — so a consumer that registers/advances while data is
//! already waiting is woken on the next update frame instead of polling.
//! Unregister removes the registry row **and** any pending wake entry.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use super::pull::{PullLimits, PullScope};
use super::Kernel;

/// Hard ceiling on simultaneously-registered pull cursors (D5: bounded).
/// Registrations past the cap (for a *new* `cursor_id`) are loud no-ops.
pub const MAX_PULL_CURSORS: usize = 128;

/// Opaque cursor handle. `0` is invalid (never armed, never stored).
///
/// The host (or the step-3b FFI wrapper) mints the id before sending the
/// fire-and-forget register command — no actor round-trip allocates it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PullCursorId(pub u64);

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
    pub consumer_id: String,
    pub scope: PullScope,
    pub mode: PullCursorMode,
    pub after_seq: u64,
    pub limits: PullLimits,
}

/// Actor-written, FFI-read-only registry of pull cursors.
#[derive(Default)]
pub struct PullCursorRegistry {
    by_id: BTreeMap<PullCursorId, PullCursorRegistration>,
}

/// Shared handle to the registry. Single writer (actor); many readers (FFI).
pub type PullCursorRegistrySlot = Arc<RwLock<PullCursorRegistry>>;

impl PullCursorRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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

    /// Register (or replace) a pull cursor — `ActorCommand::RegisterPullCursor`.
    ///
    /// Replace-by-`cursor_id` is always allowed (it does not grow the set). A
    /// *new* registration past `MAX_PULL_CURSORS` is a loud no-op (log, never
    /// panic). Arms an immediate wake when `after_seq < latest_ingest_seq`.
    pub(crate) fn register_pull_cursor(
        &mut self,
        cursor_id: PullCursorId,
        consumer_id: String,
        scope: PullScope,
        mode: PullCursorMode,
        after_seq: u64,
        limits: PullLimits,
    ) {
        if cursor_id.0 == 0 {
            tracing::warn!("RegisterPullCursor ignored: cursor_id 0 is invalid");
            return;
        }
        {
            let registry = Arc::clone(&self.pull_cursor_registry);
            let mut reg = registry.write().expect("pull cursor registry poisoned");
            let is_replace = reg.by_id.contains_key(&cursor_id);
            if !is_replace && reg.len() >= MAX_PULL_CURSORS {
                tracing::warn!(
                    cursor_id = cursor_id.0,
                    max = MAX_PULL_CURSORS,
                    "RegisterPullCursor ignored: MAX_PULL_CURSORS reached (loud no-op)"
                );
                return;
            }
            reg.by_id.insert(
                cursor_id,
                PullCursorRegistration {
                    cursor_id,
                    consumer_id,
                    scope,
                    mode,
                    after_seq,
                    limits,
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
