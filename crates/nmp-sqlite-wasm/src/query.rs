//! Scan / streaming-query read paths for the OPFS-SQLite engine (#1007 PR-4).
//!
//! The pure `(sql, params)` builders live in [`sql`] (target-agnostic,
//! native-tested); this module is the thin wasm face that binds the params,
//! steps the statement, and decodes rows. All inherent methods on
//! [`OpfsSqliteStore`] — no `EventStore` trait impl (that wrapper lands in
//! `nmp-store` in a later PR, so no stub ever ships here).
//!
//! ## Surface (matches `nmp-store/src/events.rs` semantics)
//!
//! Materializing scans (`Vec<StoredEngineEvent>`, newest-first):
//! [`OpfsSqliteStore::scan_by_author_kind`],
//! [`OpfsSqliteStore::scan_by_authors_kind`] (globally
//! `(created_at desc, id asc)`-ordered across authors; the single `events`
//! table makes it inherently dedup'd), [`OpfsSqliteStore::scan_by_kind_time`]
//! (empty `kinds` = all kinds), [`OpfsSqliteStore::scan_by_kind_dtag`],
//! [`OpfsSqliteStore::scan_by_tags`] (index-served AND/OR tag intersection), and
//! [`OpfsSqliteStore::scan_expiring_before`] (ascending NIP-40 reaper).
//!
//! Streaming: [`OpfsSqliteStore::query_visit`] seeks the ordered index
//! (`ORDER BY … DESC LIMIT ?` → O(log n) seek + O(budget) step, never an O(N)
//! scan) and invokes the visitor for every scanned row up to `budget`,
//! consuming one unit of budget per visitor call and stopping early on
//! [`ControlFlow::Break`] — mirroring the cache-serve tick budget
//! (`nmp-core/.../cache_serve/continuation.rs`: each visited row is one unit of
//! actor work).

mod sql;

/// The crate-local read query (mirror of `nmp_store::StoreQuery`) accepted by
/// [`OpfsSqliteStore::query_visit`]. Re-exported at the crate root.
pub use sql::EngineQuery;

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use core::ops::ControlFlow;
    use std::collections::{BTreeMap, BTreeSet};

    use super::sql::{self, EngineQuery, OwnedVal};
    use crate::conv::{self, StoredEngineEvent};
    use crate::error::SqliteWasmError;
    use crate::outcome::PubKey;
    use crate::shim::SqliteStmt;
    use crate::OpfsSqliteStore;

    impl OpfsSqliteStore {
        /// `idx_events_akci` scan, newest-first. Empty `kinds` yields an empty
        /// `Vec` (a positive `(author, kinds)` selection, never a wildcard).
        pub fn scan_by_author_kind(
            &self,
            author: &PubKey,
            kinds: &[u32],
            since: Option<u64>,
            until: Option<u64>,
            limit: usize,
        ) -> Result<Vec<StoredEngineEvent>, SqliteWasmError> {
            self.collect(sql::build_author_kind(author, kinds, since, until, limit))
        }

        /// `idx_events_aci`/`akci` multi-author scan, globally
        /// `(created_at desc, id asc)`-ordered across the combined author set and
        /// deduplicated (one row per id in the single `events` table). Empty
        /// `authors` or empty `kinds` yields an empty `Vec`.
        pub fn scan_by_authors_kind(
            &self,
            authors: &BTreeSet<PubKey>,
            kinds: &[u32],
            since: Option<u64>,
            until: Option<u64>,
            limit: usize,
        ) -> Result<Vec<StoredEngineEvent>, SqliteWasmError> {
            self.collect(sql::build_authors_kind(authors, kinds, since, until, limit))
        }

        /// `idx_events_kci` scan, newest-first. **Empty `kinds` scans all kinds**
        /// (`idx_events_ci`) — the only scan where empty is the "any" wildcard.
        pub fn scan_by_kind_time(
            &self,
            kinds: &[u32],
            since: Option<u64>,
            until: Option<u64>,
            limit: usize,
        ) -> Result<Vec<StoredEngineEvent>, SqliteWasmError> {
            self.collect(Some(sql::build_kind_time(kinds, since, until, limit)))
        }

        /// `idx_events_kind_dtag` scan, newest-first across all authors for
        /// `(kind, d_tag)`.
        pub fn scan_by_kind_dtag(
            &self,
            kind: u32,
            d_tag: &[u8],
            since: Option<u64>,
            until: Option<u64>,
            limit: usize,
        ) -> Result<Vec<StoredEngineEvent>, SqliteWasmError> {
            self.collect(Some(sql::build_kind_dtag(kind, d_tag, since, until, limit)))
        }

        /// Generic single-letter tag scan, newest-first — AND across letters, OR
        /// within a letter's values, served by the `event_tags` tci/atci/ktci
        /// indexes (never a full table scan). Empty `authors` = any author; empty
        /// `kinds` = any kind; an empty `tags` map (or any empty value set) yields
        /// an empty `Vec`.
        pub fn scan_by_tags(
            &self,
            authors: &BTreeSet<PubKey>,
            kinds: &[u32],
            tags: &BTreeMap<char, BTreeSet<String>>,
            since: Option<u64>,
            until: Option<u64>,
            limit: usize,
        ) -> Result<Vec<StoredEngineEvent>, SqliteWasmError> {
            self.collect(sql::build_tags(authors, kinds, tags, since, until, limit))
        }

        /// `idx_events_expires` ascending scan — the NIP-40 reaper path. Yields
        /// events whose `expires_at < unix_seconds`, oldest-expiry first.
        pub fn scan_expiring_before(
            &self,
            unix_seconds: u64,
            limit: usize,
        ) -> Result<Vec<StoredEngineEvent>, SqliteWasmError> {
            self.collect(Some(sql::build_expiring_before(unix_seconds, limit)))
        }

        /// Streaming query — invoke `visitor` once per matching event,
        /// newest-first, up to `budget` events. Seeks the ordered index
        /// (`ORDER BY … DESC LIMIT ?`: O(log n) seek + O(budget) step, never an
        /// O(N) scan) and consumes one unit of budget per visitor call; the
        /// visitor returns [`ControlFlow::Break`] to stop early without scanning
        /// the remaining budget.
        pub fn query_visit(
            &self,
            query: &EngineQuery,
            budget: usize,
            visitor: &mut dyn FnMut(&StoredEngineEvent) -> ControlFlow<()>,
        ) -> Result<(), SqliteWasmError> {
            let Some((sql, params)) = sql::build_query(query, budget) else {
                return Ok(()); // a "matches nothing" shape — visit no rows.
            };
            let conn = self.db.borrow();
            let stmt = conn.prepare(&sql)?;
            bind_owned(&stmt, &params)?;
            // `budget` is the SQL `LIMIT`, so the step loop self-terminates; the
            // explicit counter makes the per-visit budget consumption legible.
            let mut remaining = budget;
            while remaining > 0 {
                if !stmt.step()? {
                    break;
                }
                let ev = decode_row(&stmt)?;
                remaining -= 1;
                if let ControlFlow::Break(()) = visitor(&ev) {
                    break;
                }
            }
            Ok(())
        }

        /// Run a built `(sql, params)` to completion, materializing every row
        /// newest-first. `None` (a "matches nothing" shape) yields an empty `Vec`
        /// without touching the connection.
        fn collect(
            &self,
            built: Option<(String, Vec<OwnedVal>)>,
        ) -> Result<Vec<StoredEngineEvent>, SqliteWasmError> {
            let Some((sql, params)) = built else {
                return Ok(Vec::new());
            };
            let conn = self.db.borrow();
            let stmt = conn.prepare(&sql)?;
            bind_owned(&stmt, &params)?;
            let mut out = Vec::new();
            while stmt.step()? {
                out.push(decode_row(&stmt)?);
            }
            Ok(out)
        }
    }

    /// Decode a `SELECT raw, received_at_ms` result row into a
    /// [`StoredEngineEvent`] (shared by the scan and streaming paths).
    fn decode_row(stmt: &SqliteStmt<'_>) -> Result<StoredEngineEvent, SqliteWasmError> {
        let blob = stmt.column_blob(0)?;
        let received_at_ms = stmt.column_int64(1)? as u64;
        let event = conv::decode_blob(&blob)?;
        Ok(StoredEngineEvent {
            event,
            received_at_ms,
        })
    }

    /// Bind owned builder params at 1-based positions in order. (The write path's
    /// [`crate::store_impl::bind_params`] binds borrowed `SqlVal`; the read
    /// builders synthesize owned values, so they get their own bind loop.)
    fn bind_owned(stmt: &SqliteStmt<'_>, params: &[OwnedVal]) -> Result<(), SqliteWasmError> {
        for (i, v) in params.iter().enumerate() {
            let idx = (i + 1) as i32;
            match v {
                OwnedVal::Int(n) => stmt.bind_int64(idx, *n)?,
                OwnedVal::Text(s) => stmt.bind_text(idx, s)?,
                OwnedVal::Blob(b) => stmt.bind_blob(idx, b)?,
            }
        }
        Ok(())
    }
}
