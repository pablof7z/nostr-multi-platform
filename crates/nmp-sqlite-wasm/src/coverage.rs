//! K3 coverage ledger + F-TTL replaceable-freshness cache (#1007 PR-5).
//!
//! Mirrors `nmp-store/src/lmdb/coverage.rs` (ADR-0056 §3). The `coverage` table
//! maps `(filter_hash, relay)` → `covered_through` (the downward-closed,
//! monotonic watermark through which a sync has COMPLETED for that shape on that
//! relay — see `nmp_store::CoverageRow`). The store is relay-agnostic but the
//! ledger is per-`(filter_hash, relay)`, which is exactly why GC's Stage-D3
//! backstop must lower the right row when it evicts a covered event.
//!
//! The [`CoverageGuard`] type is pure and target-agnostic (it carries a
//! kernel-owned shape-match closure, no shim), so it compiles on native; the
//! ledger read/write methods are wasm-gated because they touch the SQLite shim.

use std::sync::Arc;

/// Predicate the [`CoverageGuard`] carries: does an event (by its store fields)
/// belong to the covered shape? Kernel-owned (the shape-match logic lives in
/// `nmp-planner`/`nmp-core`, D0), passed opaquely so the store never links shape
/// logic. Args: `(event_id_hex, author_hex, kind, created_at, tags)`.
pub type CoverageMatchFn =
    Arc<dyn Fn(&str, &str, u32, u64, &[Vec<String>]) -> bool + Send + Sync>;

/// K3 Stage D3 (ADR-0056 §3.D3) — the eviction⇄ledger coherence backstop input.
///
/// One guard per active covered `(filter_hash, relay)`. When LRU eviction
/// deletes an event the guard `matches` whose `created_at <= covered_through`,
/// the store lowers that row's `covered_through` to just below the oldest
/// evicted covered event **in the same transaction** as the delete, so the
/// ledger never claims a range it no longer holds. An empty guard slice makes
/// the eviction path byte-identical to the pin-only path.
#[derive(Clone)]
pub struct CoverageGuard {
    /// Canonical filter hash half of the ledger key.
    pub filter_hash: String,
    /// Relay half of the ledger key.
    pub relay: String,
    /// The downward-closed coverage bound this guard protects.
    pub covered_through: u64,
    /// Does an event (by store fields) match the covered shape? (kernel-owned).
    pub matches: CoverageMatchFn,
}

impl std::fmt::Debug for CoverageGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoverageGuard")
            .field("filter_hash", &self.filter_hash)
            .field("relay", &self.relay)
            .field("covered_through", &self.covered_through)
            .field("matches", &"<fn>")
            .finish()
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm_impl::lower_guards_in_txn;

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use super::CoverageGuard;
    use crate::error::SqliteWasmError;
    use crate::shim::SqliteConn;
    use crate::store_impl::{exec_write, SqlVal};
    use crate::types::ReplaceableKey;
    use crate::OpfsSqliteStore;

    impl OpfsSqliteStore {
        /// Advance the `(filter_hash, relay)` watermark to
        /// `max(existing, covered_through)`. Monotonic by construction; a
        /// `covered_through == 0` call is a no-op (empty ledger ≡ a 0-row).
        pub fn record_coverage(
            &self,
            filter_hash: &str,
            relay: &str,
            covered_through: u64,
        ) -> Result<(), SqliteWasmError> {
            // INSERT-or-bump in one statement: the row only moves up, never down.
            let conn = self.db.borrow();
            exec_write(
                &conn,
                "INSERT INTO coverage (filter_hash, relay, covered_through)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(filter_hash, relay) DO UPDATE SET
                     covered_through = MAX(covered_through, excluded.covered_through)",
                &[
                    SqlVal::Text(filter_hash),
                    SqlVal::Text(relay),
                    SqlVal::Int(covered_through as i64),
                ],
            )
        }

        /// Read the `(filter_hash, relay)` watermark, or `None` if no row exists.
        pub fn get_coverage(
            &self,
            filter_hash: &str,
            relay: &str,
        ) -> Result<Option<u64>, SqliteWasmError> {
            let conn = self.db.borrow();
            let stmt = conn.prepare(
                "SELECT covered_through FROM coverage WHERE filter_hash = ?1 AND relay = ?2",
            )?;
            stmt.bind_text(1, filter_hash)?;
            stmt.bind_text(2, relay)?;
            if stmt.step()? {
                Ok(Some(stmt.column_int64(0)? as u64))
            } else {
                Ok(None)
            }
        }

        /// The highest `covered_through` recorded for `filter_hash` across ALL
        /// relays, or `None`. The floor-coherent pin set protects below this MAX
        /// (over-pinning only defers eviction; under-pinning punches a hole).
        pub fn coverage_max_for_filter_hash(
            &self,
            filter_hash: &str,
        ) -> Result<Option<u64>, SqliteWasmError> {
            let conn = self.db.borrow();
            // MAX() over the filter_hash group — `idx_coverage_fh` serves it.
            let stmt = conn
                .prepare("SELECT MAX(covered_through) FROM coverage WHERE filter_hash = ?1")?;
            stmt.bind_text(1, filter_hash)?;
            if stmt.step()? {
                // MAX over an empty group is SQL NULL → a 0-length blob through
                // the shim's int reader returns 0; distinguish "no rows" with a
                // COUNT-free guard: a NULL aggregate means no matching row.
                let any = conn.prepare("SELECT 1 FROM coverage WHERE filter_hash = ?1 LIMIT 1")?;
                any.bind_text(1, filter_hash)?;
                if any.step()? {
                    return Ok(Some(stmt.column_int64(0)? as u64));
                }
            }
            Ok(None)
        }

        /// Every `(relay, covered_through)` row recorded for `filter_hash`, in
        /// arbitrary order — the kernel builds one [`CoverageGuard`] per row.
        pub fn coverage_rows_for_filter_hash(
            &self,
            filter_hash: &str,
        ) -> Result<Vec<(String, u64)>, SqliteWasmError> {
            let conn = self.db.borrow();
            let stmt = conn
                .prepare("SELECT relay, covered_through FROM coverage WHERE filter_hash = ?1")?;
            stmt.bind_text(1, filter_hash)?;
            let mut out = Vec::new();
            while stmt.step()? {
                out.push((stmt.column_text(0)?, stmt.column_int64(1)? as u64));
            }
            Ok(out)
        }

        // ─── F-TTL replaceable freshness ──────────────────────────────────────

        /// Read the `check_again_after` (unix ms) for a replaceable identity, or
        /// `None` if it was never freshness-stamped (the TTL gate treats `None`
        /// as "due now").
        pub fn get_check_again_after(
            &self,
            key: &ReplaceableKey,
        ) -> Result<Option<u64>, SqliteWasmError> {
            let rkey = key.encode();
            let conn = self.db.borrow();
            let stmt = conn
                .prepare("SELECT check_again_after FROM replaceable_freshness WHERE rkey = ?1")?;
            stmt.bind_blob(1, &rkey)?;
            if stmt.step()? {
                Ok(Some(stmt.column_int64(0)? as u64))
            } else {
                Ok(None)
            }
        }

        /// Stamp the `check_again_after` (unix ms) for a replaceable identity.
        pub fn set_check_again_after(
            &self,
            key: &ReplaceableKey,
            ts_ms: u64,
        ) -> Result<(), SqliteWasmError> {
            let rkey = key.encode();
            let conn = self.db.borrow();
            exec_write(
                &conn,
                "INSERT INTO replaceable_freshness (rkey, check_again_after)
                 VALUES (?1, ?2)
                 ON CONFLICT(rkey) DO UPDATE SET check_again_after = excluded.check_again_after",
                &[SqlVal::Blob(&rkey), SqlVal::Int(ts_ms as i64)],
            )
        }
    }

    /// Stage-D3 backstop — lower each guard's coverage row to just below the
    /// oldest evicted covered event for it, INSIDE the supplied GC write txn (so
    /// the lowering commits atomically with the deletes). `new_bound =
    /// oldest_evicted - 1`; `0` clears the row. `min_evicted_covered[gi] == None`
    /// means guard `gi` stranded nothing this pass (row untouched).
    pub(crate) fn lower_guards_in_txn(
        conn: &SqliteConn,
        guards: &[CoverageGuard],
        min_evicted_covered: &[Option<u64>],
    ) -> Result<(), SqliteWasmError> {
        for (gi, guard) in guards.iter().enumerate() {
            let Some(oldest_evicted) = min_evicted_covered[gi] else {
                continue;
            };
            let new_bound = oldest_evicted.saturating_sub(1);
            lower_one(conn, &guard.filter_hash, &guard.relay, new_bound)?;
        }
        Ok(())
    }

    /// Lower one row to `new_bound` (downward-only). No-op when the row is absent
    /// or already at/below `new_bound`; `new_bound == 0` deletes the row rather
    /// than leave a misleading 0-bound. Shares the caller's open txn.
    fn lower_one(
        conn: &SqliteConn,
        filter_hash: &str,
        relay: &str,
        new_bound: u64,
    ) -> Result<(), SqliteWasmError> {
        let existing = {
            let stmt = conn.prepare(
                "SELECT covered_through FROM coverage WHERE filter_hash = ?1 AND relay = ?2",
            )?;
            stmt.bind_text(1, filter_hash)?;
            stmt.bind_text(2, relay)?;
            if stmt.step()? {
                Some(stmt.column_int64(0)? as u64)
            } else {
                None
            }
        };
        let Some(existing) = existing else {
            return Ok(()); // no row to lower
        };
        if existing <= new_bound {
            return Ok(()); // already honest — never raise
        }
        if new_bound == 0 {
            exec_write(
                conn,
                "DELETE FROM coverage WHERE filter_hash = ?1 AND relay = ?2",
                &[SqlVal::Text(filter_hash), SqlVal::Text(relay)],
            )
        } else {
            exec_write(
                conn,
                "UPDATE coverage SET covered_through = ?3 WHERE filter_hash = ?1 AND relay = ?2",
                &[
                    SqlVal::Text(filter_hash),
                    SqlVal::Text(relay),
                    SqlVal::Int(new_bound as i64),
                ],
            )
        }
    }
}
