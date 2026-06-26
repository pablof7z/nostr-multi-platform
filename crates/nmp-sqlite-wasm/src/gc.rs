//! Bounded-store garbage collection for the OPFS-SQLite engine (#1007 PR-5).
//!
//! Mirrors `nmp-store/src/lmdb/gc.rs`, scoped to what this engine owns. Three
//! phases, each bounded by `budget.max_events_per_step`:
//!
//!   * **Phase 1 — NIP-40 expiry reap.** `idx_events_expires` scan for
//!     `expires_at <= now_secs`; each reaped event is removed everywhere it
//!     lives, a `NIP40Expiry` tombstone is written (so a re-arrival is suppressed
//!     by the same path PR-3's insert already honours), and a
//!     `Deleted{Nip40Expiry}` ingest-log entry is appended.
//!   * **Phase 2 — LRU eviction** (only when `budget.max_total_events` is
//!     finite). Least-recently-accessed un-pinned events are removed toward the
//!     ceiling. No tombstone / no ingest-log row — an evicted event is still a
//!     valid Nostr event that may be re-fetched. K3 Stage-D3 backstop: if
//!     eviction deletes an event a [`CoverageGuard`] matches at/below its
//!     `covered_through`, that ledger row is lowered **in the same transaction**.
//!   * **Phase 3 — tombstone purge.** Per-id and address tombstones older than
//!     90 days are dropped, throttled to once per hour using the caller-supplied
//!     `now_secs` (D7 — the store never reads the clock itself).
//!
//! No wall-time budget: `wasm32-unknown-unknown` has no usable monotonic clock
//! (`Instant::now()` is unimplemented; reading one would be a D7 surface), so GC
//! is bounded purely by `max_events_per_step` and `GcReport::duration_ms` is 0.
//! LRU order: `get_by_id` / `hot_set_hint` stamp a fresh `lru_seq` into
//! `lru_access`; eviction orders by `COALESCE(seq, 0)` then arrival, so
//! never-accessed events sort oldest (no insert-path stamping; `peek` stays
//! write-free).

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm_impl::stamp_lru;

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use std::collections::HashSet;

    use crate::conv::{self, EngineEvent};
    use crate::coverage::{lower_guards_in_txn, CoverageGuard};
    use crate::delete::remove_event;
    use crate::error::SqliteWasmError;
    use crate::ingest_log::{append_deleted, DeleteReason};
    use crate::meta::{self, KEY_LRU_SEQ};
    use crate::outcome::EventId;
    use crate::shim::SqliteConn;
    use crate::store_impl::{blob32, exec_write, with_txn, SqlVal};
    use crate::types::{GcBudget, GcReport};
    use crate::OpfsSqliteStore;

    impl OpfsSqliteStore {
        /// One bounded GC pass with an explicit derived pin set (no coverage
        /// backstop). Convenience over [`Self::gc_step_with_pins_and_coverage`]
        /// with an empty guard slice.
        pub fn gc_step_with_pins(
            &self,
            budget: GcBudget,
            now_secs: u64,
            pins: &HashSet<EventId>,
        ) -> Result<GcReport, SqliteWasmError> {
            self.gc_step_with_pins_and_coverage(budget, now_secs, pins, &[])
        }

        /// One bounded GC pass with both a pin set AND the K3 Stage-D3
        /// eviction⇄ledger coherence backstop. With an empty `guards` slice this
        /// is byte-identical to [`Self::gc_step_with_pins`].
        pub fn gc_step_with_pins_and_coverage(
            &self,
            budget: GcBudget,
            now_secs: u64,
            pins: &HashSet<EventId>,
            guards: &[CoverageGuard],
        ) -> Result<GcReport, SqliteWasmError> {
            let mut report = GcReport::default();
            let conn = self.db.borrow();

            phase1_reap_expired(&conn, &budget, now_secs, &mut report)?;
            phase2_evict_lru(&conn, &budget, pins, guards, &mut report)?;
            crate::gc_tombstones::purge_tombstones(&conn, now_secs, &mut report)?;

            Ok(report)
        }

        /// Soft hint: stamp these ids as most-recently-used so GC evicts them
        /// last. Best-effort (the whole hint is one transaction).
        pub fn hot_set_hint(&self, ids: &[EventId]) -> Result<(), SqliteWasmError> {
            if ids.is_empty() {
                return Ok(());
            }
            let conn = self.db.borrow();
            with_txn(&conn, |c| {
                for id in ids {
                    stamp_lru(c, id)?;
                }
                Ok(())
            })
        }
    }

    /// Stamp a fresh access sequence for `id` (point-read / hot-set LRU bump).
    /// Runs inside the caller's write txn.
    pub(crate) fn stamp_lru(conn: &SqliteConn, id: &EventId) -> Result<(), SqliteWasmError> {
        let seq = meta::bump_u64(conn, KEY_LRU_SEQ)?;
        exec_write(
            conn,
            "INSERT INTO lru_access (event_id, seq) VALUES (?1, ?2)
             ON CONFLICT(event_id) DO UPDATE SET seq = excluded.seq",
            &[SqlVal::Blob(id), SqlVal::Int(seq as i64)],
        )
    }

    /// Drop an event's LRU access row (called from every removal path here).
    fn lru_delete(conn: &SqliteConn, id: &EventId) -> Result<(), SqliteWasmError> {
        exec_write(
            conn,
            "DELETE FROM lru_access WHERE event_id = ?1",
            &[SqlVal::Blob(id)],
        )
    }

    // ─── Phase 1 ────────────────────────────────────────────────────────────

    fn phase1_reap_expired(
        conn: &SqliteConn,
        budget: &GcBudget,
        now_secs: u64,
        report: &mut GcReport,
    ) -> Result<(), SqliteWasmError> {
        // Collect expired ids (lowest expiry first) under the read borrow, then
        // delete them in one txn — never mutate a table mid-cursor.
        let victims: Vec<EventId> = {
            let stmt = conn.prepare(
                "SELECT id FROM events
                 WHERE expires_at IS NOT NULL AND expires_at <= ?1
                 ORDER BY expires_at ASC LIMIT ?2",
            )?;
            stmt.bind_int64(1, now_secs as i64)?;
            stmt.bind_int64(2, budget.max_events_per_step as i64)?;
            let mut out = Vec::new();
            while stmt.step()? {
                if let Some(id) = blob32(&stmt.column_blob(0)?) {
                    out.push(id);
                }
            }
            out
        };
        if victims.is_empty() {
            return Ok(());
        }
        with_txn(conn, |c| {
            for id in &victims {
                remove_event(c, id)?;
                lru_delete(c, id)?;
                // NIP40Expiry tombstone — suppresses a later re-arrival via the
                // same per-id check PR-3's insert already applies.
                exec_write(
                    c,
                    "INSERT INTO tombstones
                         (target_id, kind5_event_id, deleter_pubkey, deleted_at, origin, source)
                     VALUES (?1, NULL, NULL, ?2, 1, 'gc')
                     ON CONFLICT(target_id) DO UPDATE SET deleted_at = excluded.deleted_at",
                    &[SqlVal::Blob(id), SqlVal::Int(now_secs as i64)],
                )?;
                append_deleted(c, id, id, DeleteReason::Nip40Expiry, now_secs * 1000)?;
                report.expired_reaped += 1;
            }
            Ok(())
        })
    }

    // ─── Phase 2 ────────────────────────────────────────────────────────────

    fn phase2_evict_lru(
        conn: &SqliteConn,
        budget: &GcBudget,
        pins: &HashSet<EventId>,
        guards: &[CoverageGuard],
        report: &mut GcReport,
    ) -> Result<(), SqliteWasmError> {
        if budget.max_total_events == usize::MAX {
            return Ok(());
        }
        let event_count = count_events(conn)?;
        if event_count <= budget.max_total_events {
            return Ok(());
        }
        let overage = event_count - budget.max_total_events;
        let to_evict = overage.min(budget.max_events_per_step);

        // Oldest-access-first candidates (un-stamped events sort as seq 0). Filter
        // pinned in Rust and stop once enough victims are gathered.
        let victims: Vec<EventId> = {
            let stmt = conn.prepare(
                "SELECT e.id FROM events e
                 LEFT JOIN lru_access l ON l.event_id = e.id
                 ORDER BY COALESCE(l.seq, 0) ASC, e.received_at_ms ASC",
            )?;
            let mut out = Vec::new();
            while stmt.step()? {
                if out.len() >= to_evict {
                    break;
                }
                if let Some(id) = blob32(&stmt.column_blob(0)?) {
                    if !pins.contains(&id) {
                        out.push(id);
                    }
                }
            }
            out
        };
        if victims.is_empty() {
            return Ok(());
        }

        with_txn(conn, |c| {
            // Per guard, the lowest created_at of an evicted event it matches
            // at/below its bound — lowered into THIS txn before commit.
            let mut min_evicted_covered: Vec<Option<u64>> = vec![None; guards.len()];
            for id in &victims {
                let fields = load_guard_fields(c, id)?;
                remove_event(c, id)?;
                lru_delete(c, id)?;
                if let Some(ev) = fields {
                    note_guard_eviction(guards, &ev, &mut min_evicted_covered);
                }
                report.lru_evicted += 1;
            }
            lower_guards_in_txn(c, guards, &min_evicted_covered)?;
            Ok(())
        })
    }

    /// Decoded fields an evicted event contributes to the guard backstop.
    struct GuardFields {
        id_hex: String,
        author_hex: String,
        kind: u32,
        created_at: u64,
        tags: Vec<Vec<String>>,
    }

    /// Load `(id_hex, author_hex, kind, created_at, tags)` for guard matching, or
    /// `None` when there are no guards (skip the decode entirely) or the row is
    /// gone. The raw blob is the canonical event JSON (see `conv`).
    fn load_guard_fields(
        conn: &SqliteConn,
        id: &EventId,
    ) -> Result<Option<GuardFields>, SqliteWasmError> {
        let stmt = conn.prepare("SELECT raw FROM events WHERE id = ?1")?;
        stmt.bind_blob(1, id)?;
        if !stmt.step()? {
            return Ok(None);
        }
        let ev: EngineEvent = conv::decode_blob(&stmt.column_blob(0)?)?;
        Ok(Some(GuardFields {
            id_hex: ev.id.clone(),
            author_hex: ev.pubkey.clone(),
            kind: ev.kind,
            created_at: ev.created_at,
            tags: ev.tags,
        }))
    }

    /// Record, per matching guard, the oldest evicted covered `created_at`.
    fn note_guard_eviction(
        guards: &[CoverageGuard],
        ev: &GuardFields,
        min_evicted_covered: &mut [Option<u64>],
    ) {
        for (gi, guard) in guards.iter().enumerate() {
            if ev.created_at <= guard.covered_through
                && (guard.matches)(&ev.id_hex, &ev.author_hex, ev.kind, ev.created_at, &ev.tags)
            {
                let slot = &mut min_evicted_covered[gi];
                *slot = Some(slot.map_or(ev.created_at, |m| m.min(ev.created_at)));
            }
        }
    }

    fn count_events(conn: &SqliteConn) -> Result<usize, SqliteWasmError> {
        let stmt = conn.prepare("SELECT COUNT(*) FROM events")?;
        if stmt.step()? {
            Ok(stmt.column_int64(0)? as usize)
        } else {
            Ok(0)
        }
    }
}
