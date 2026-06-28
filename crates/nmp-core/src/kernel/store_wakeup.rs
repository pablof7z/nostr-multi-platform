//! Event-driven store-wakeup subsystem (ADR-0058 §10, step 3a).
//!
//! The single actor-owned wake source. Generalizes the #1520 event-driven
//! cache-serve wakeup into one structure that carries **both** wake arms:
//!
//! - `cache_serve`: the #1520 set of already-served interest completion keys
//!   that a live insert re-armed (behavior preserved **byte-for-byte**).
//! - `pull`: ADR-0058 pull-cursor wakes, coalesced to `cursor_id -> latest_seq`.
//!
//! `Kernel` owns exactly **one** [`StoreWakeups`]. There is no separate
//! `pull_wakeups` field, no channel, no callback, no timer — D8 (no polling):
//! both arms are armed at the one ingest chokepoint and drained on the existing
//! actor cadence.
//!
//! ## The one chokepoint
//!
//! [`Kernel::note_store_mutation`] replaces the old `note_store_insert`. It is
//! called from the SAME post-store-mutation site in `ingest/accepted.rs` and
//! does both arms in one method:
//!
//! 1. **cache-serve arm** — identical to #1520: for each active interest whose
//!    shape matches the event AND whose completion key is already in
//!    `served_interest_shapes`, insert the completion key into
//!    `store_wakeups.cache_serve`.
//! 2. **pull arm** — if any pull cursor is registered, read
//!    `latest_ingest_seq` and, for every cursor with `after_seq < latest_seq`,
//!    set `store_wakeups.pull[cursor_id] = max(existing, latest_seq)`. Multiple
//!    ingest-log rows from one mutation (e.g. kind:5) coalesce to one latest
//!    seq per cursor.
//!
//! ## Drain paths
//!
//! - `Kernel::drain_cache_serve_wakeups` — drains `cache_serve` exactly as
//!   #1520 did (first action of `run_cache_serve_step`). It lives in
//!   `kernel::cache_serve::wakeup` (next to the sealed-private re-enqueue
//!   helper it calls), not here.
//! - [`Kernel::drain_pull_wakes`] — drains `pull` into a `Vec<(PullCursorId,
//!   u64)>` for emit, then re-arms any cursor still behind `latest_seq`
//!   (level-triggered). The transport emission of that batch (the
//!   `nmp.pull.wake` sidecar) is step 3b and is not wired here.

use std::collections::{BTreeMap, BTreeSet};

use super::pull_cursor::PullCursorId;
use super::Kernel;

/// The single actor-owned store-wakeup state (both arms).
#[derive(Default)]
pub(in crate::kernel) struct StoreWakeups {
    /// #1520 cache-serve re-arm set — already-served interest completion keys.
    /// `BTreeSet` coalesces N rapid inserts for one interest to ONE entry.
    pub(in crate::kernel) cache_serve: BTreeSet<u64>,
    /// ADR-0058 pull-cursor wakes — coalesced `cursor_id -> latest_seq`.
    pub(in crate::kernel) pull: BTreeMap<PullCursorId, u64>,
}

impl StoreWakeups {
    #[must_use]
    pub(in crate::kernel) fn new() -> Self {
        Self::default()
    }
}

impl Kernel {
    /// Record that a live store mutation matched active interests and/or
    /// advanced the ingest log. The single post-store-mutation chokepoint
    /// (replaces `note_store_insert`).
    ///
    /// Called from the canonical accepted-event path in `ingest/accepted.rs`
    /// (after `project_accepted_event`) for `Inserted | Replaced | Ephemeral`
    /// outcomes. Performs BOTH wake arms (see module doc).
    pub(in crate::kernel) fn note_store_mutation(
        &mut self,
        event_id: &str,
        author: &str,
        kind: u32,
        created_at: u64,
        tags: &[Vec<String>],
        store_log_advanced: bool,
    ) {
        // ── cache-serve arm (#1520 — byte-for-byte preserved) ─────────────────
        // Snapshot the active interests so we can borrow self.served_interest_shapes
        // without a split-borrow conflict.
        let active = self.lifecycle.registry().iter_active_with_keys();
        for (sub_key, interest) in &active {
            if interest
                .shape
                .matches_event_with_id(event_id, author, kind, created_at, tags)
            {
                let key = super::cache_serve::completion_key_for_interest(sub_key, &interest.shape);
                if self.served_interest_shapes.contains(&key) {
                    self.store_wakeups.cache_serve.insert(key);
                }
            }
        }

        // ── pull arm (ADR-0058) ───────────────────────────────────────────────
        // Only arm pull wakes when the ingest log actually advanced
        // (`Inserted | Replaced`). Ephemerals match interests for the cache-serve
        // arm above but are never stored and never advance `latest_ingest_seq`,
        // so they must not trigger a pull re-arm (ADR §3: wakes ride log append).
        // Also skip the store read entirely when no cursor is registered.
        if store_log_advanced {
            let any_cursor = !self
                .pull_cursor_registry
                .read()
                .expect("pull cursor registry poisoned")
                .is_empty();
            if any_cursor {
                self.rearm_pull_wakes_still_behind();
            }
        }
    }

    /// Drain the coalesced pull-cursor wakes into a batch for emit, then re-arm
    /// any cursor still behind `latest_ingest_seq` (the level-triggered
    /// contract, ADR §3 / §6.1).
    ///
    /// Returns `Vec<(PullCursorId, latest_seq)>`. The re-arm reinserts into the
    /// SAME `store_wakeups.pull` map — it is not a second wake path. A consumer
    /// clears its wake by advancing its cursor (`AdvancePullCursor`) so it is no
    /// longer behind; an un-advanced cursor is deliberately re-woken next frame
    /// so it drains rather than sleep-rechecks.
    ///
    /// The transport emission of this batch (`nmp.pull.wake` sidecar) lands in
    /// `make_update` just before `merge_builtin_typed_projections` (step 3b).
    pub(in crate::kernel) fn drain_pull_wakes(&mut self) -> Vec<(PullCursorId, u64)> {
        if self.store_wakeups.pull.is_empty() {
            return Vec::new();
        }
        let drained: Vec<(PullCursorId, u64)> = std::mem::take(&mut self.store_wakeups.pull)
            .into_iter()
            .collect();
        // Level-triggered: re-arm cursors still behind the head.
        self.rearm_pull_wakes_still_behind();
        drained
    }

    /// Whether the cache-serve wake arm has coalesced keys waiting. The actor
    /// loop checks this alongside `has_pending_cache_serves` to decide whether
    /// to call `run_cache_serve_step`. (Live via that loop in default/native
    /// builds; the loop is gated out of the wasm build — see the analogous note
    /// in `pull_cursor.rs`.)
    // `allow(dead_code)`: live via the native actor loop; that loop is
    // feature-gated out of the wasm build so the method appears dead there.
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn has_cache_serve_wakeups(&self) -> bool {
        !self.store_wakeups.cache_serve.is_empty()
    }

    /// Whether either wake arm has work pending. (Used by tests today; the
    /// actor loop gate adopts it alongside the pull emit seam in step 3b.)
    // `allow(dead_code)`: used by tests; the actor loop adoption lands in step 3b.
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn has_store_wakeups(&self) -> bool {
        !self.store_wakeups.cache_serve.is_empty() || !self.store_wakeups.pull.is_empty()
    }
}
