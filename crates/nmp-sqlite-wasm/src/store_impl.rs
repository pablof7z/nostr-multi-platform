//! `OpfsSqliteStore` inherent impl: async `open` (now schema-creating), the
//! transaction + prepared-statement helpers the write path shares, the point
//! reads, and the single scoped `unsafe impl Send + Sync` (ADR-0054 §3).
//!
//! The `EventStore` trait impl lives in `nmp-store`, not here (#1007): this PR
//! adds only real, complete inherent methods — no `todo!()`/`unimplemented!()`
//! stub ever ships inside a trait impl (zero-tolerance no-hacks rule).

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm_impl::{blob32, exec_write, with_txn, SqlVal};

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use crate::conv::{self, StoredEngineEvent};
    use crate::error::SqliteWasmError;
    use crate::outcome::{EventId, PubKey};
    use crate::schema;
    use crate::shim::{SqliteConn, SqliteStmt};
    use crate::OpfsSqliteStore;
    use core::cell::RefCell;

    /// A bound parameter value for [`exec_write`] / [`bind_params`].
    pub(crate) enum SqlVal<'a> {
        /// 64-bit integer.
        Int(i64),
        /// UTF-8 text.
        Text(&'a str),
        /// Byte blob.
        Blob(&'a [u8]),
        /// SQL NULL.
        Null,
    }

    impl OpfsSqliteStore {
        /// Open the OPFS-SQLite store named `database_name` and ensure its schema.
        ///
        /// This is the one-time async pool-open entry point (ADR-0054 §1):
        /// initialise the SQLite WASM module, install the opfs-sahpool VFS, open
        /// the database, then create/migrate the schema. Every later store
        /// operation is synchronous over the returned handle.
        pub async fn open(database_name: &str) -> Result<Self, SqliteWasmError> {
            let conn = SqliteConn::open(database_name).await?;
            schema::ensure_schema(&conn)?;
            Ok(Self {
                db: RefCell::new(conn),
            })
        }

        /// Borrow the underlying connection cell.
        ///
        /// The `RefCell` enforces single-borrow discipline at runtime within the
        /// owning Worker actor (ADR-0054 §3 — `RefCell`, not `Mutex`).
        pub fn conn(&self) -> &RefCell<SqliteConn> {
            &self.db
        }

        // ─── Point reads ──────────────────────────────────────────────────────

        /// Primary lookup by id. `Ok(None)` if absent (a tombstoned event is not
        /// present — deletion removes the primary row).
        ///
        /// Read-only. (The `EventStore` contract has `get_by_id` stamp the LRU;
        /// there is no LRU until the GC PR lands, so this is currently identical
        /// to [`Self::peek_by_id`]. The LRU stamp will be added here — and only
        /// here — when GC arrives, keeping `peek_by_id` write-free.)
        pub fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEngineEvent>, SqliteWasmError> {
            self.read_stored(
                "SELECT raw, received_at_ms FROM events WHERE id = ?1",
                &[SqlVal::Blob(id)],
            )
        }

        /// Pure point-read by id — never opens a write transaction, never stamps
        /// any LRU/GC state. Use on replay paths that must not bias eviction.
        pub fn peek_by_id(
            &self,
            id: &EventId,
        ) -> Result<Option<StoredEngineEvent>, SqliteWasmError> {
            self.read_stored(
                "SELECT raw, received_at_ms FROM events WHERE id = ?1",
                &[SqlVal::Blob(id)],
            )
        }

        /// Current parameterized-replaceable for `(pubkey, kind, d_tag)`, newest
        /// wins, or `Ok(None)`. Index-served by `idx_events_dtag`.
        pub fn get_param_replaceable(
            &self,
            pubkey: &PubKey,
            kind: u32,
            d_tag: &[u8],
        ) -> Result<Option<StoredEngineEvent>, SqliteWasmError> {
            self.read_stored(
                "SELECT raw, received_at_ms FROM events
                 WHERE pubkey = ?1 AND kind = ?2 AND d_tag = ?3
                 ORDER BY created_at DESC LIMIT 1",
                &[
                    SqlVal::Blob(pubkey),
                    SqlVal::Int(i64::from(kind)),
                    SqlVal::Blob(d_tag),
                ],
            )
        }

        /// Run a single-row `SELECT raw, received_at_ms` and decode it.
        fn read_stored(
            &self,
            sql: &str,
            params: &[SqlVal<'_>],
        ) -> Result<Option<StoredEngineEvent>, SqliteWasmError> {
            let conn = self.db.borrow();
            let stmt = conn.prepare(sql)?;
            bind_params(&stmt, params)?;
            if stmt.step()? {
                let blob = stmt.column_blob(0)?;
                let received_at_ms = stmt.column_int64(1)? as u64;
                let event = conv::decode_blob(&blob)?;
                Ok(Some(StoredEngineEvent {
                    event,
                    received_at_ms,
                }))
            } else {
                Ok(None)
            }
        }
    }

    /// Bind `params` to `stmt` at 1-based positions in order.
    pub(crate) fn bind_params(
        stmt: &SqliteStmt<'_>,
        params: &[SqlVal<'_>],
    ) -> Result<(), SqliteWasmError> {
        for (i, v) in params.iter().enumerate() {
            let idx = (i + 1) as i32;
            match v {
                SqlVal::Int(n) => stmt.bind_int64(idx, *n)?,
                SqlVal::Text(s) => stmt.bind_text(idx, s)?,
                SqlVal::Blob(b) => stmt.bind_blob(idx, b)?,
                SqlVal::Null => stmt.bind_null(idx)?,
            }
        }
        Ok(())
    }

    /// Normalize a column blob to a 32-byte id/pubkey, or `None`. A NULL column
    /// reads back as an empty blob through the shim, so any non-32 length is
    /// treated as absent.
    pub(crate) fn blob32(bytes: &[u8]) -> Option<[u8; 32]> {
        bytes.try_into().ok()
    }

    /// Prepare `sql`, bind `params`, and drive the statement to completion
    /// (no result rows expected — INSERT / UPDATE / DELETE).
    pub(crate) fn exec_write(
        conn: &SqliteConn,
        sql: &str,
        params: &[SqlVal<'_>],
    ) -> Result<(), SqliteWasmError> {
        let stmt = conn.prepare(sql)?;
        bind_params(&stmt, params)?;
        while stmt.step()? {}
        Ok(())
    }

    /// Run `f` inside a single SQLite transaction.
    ///
    /// OPFS gives no write atomicity on its own; the SQLite `BEGIN`/`COMMIT`
    /// supplies it, so the primary row + every secondary-index row + provenance +
    /// ingest-log entry + tombstone side-effects of one insert either all land or
    /// all roll back (the `EventStore::insert` atomicity contract). On any error
    /// the txn is rolled back and the error propagated.
    pub(crate) fn with_txn<T>(
        conn: &SqliteConn,
        f: impl FnOnce(&SqliteConn) -> Result<T, SqliteWasmError>,
    ) -> Result<T, SqliteWasmError> {
        conn.exec("BEGIN")?;
        match f(conn) {
            Ok(value) => {
                conn.exec("COMMIT")?;
                Ok(value)
            }
            Err(e) => {
                // Best-effort rollback; surface the original fault regardless.
                let _ = conn.exec("ROLLBACK");
                Err(e)
            }
        }
    }

    // SAFETY: `OpfsSqliteStore` wraps a `RefCell<SqliteConn>` whose `JsValue`
    // engine handle is `!Send + !Sync`. The store is constructed inside, and
    // only ever observed by, the single Web Worker event loop that opened its
    // OPFS SyncAccessHandle pool (ADR-0047 §1: the Worker IS the actor; D4
    // single writer). No other thread ever obtains a reference to the handle.
    // The `target_feature = "atomics"` `compile_error!` guard in the crate root
    // forbids the only build configuration (wasm threads) that could introduce
    // a second thread and make this impl unsound. This is the ONLY `unsafe` in
    // the crate and is forbidden anywhere outside it (ADR-0054 §3).
    unsafe impl Send for OpfsSqliteStore {}
    // SAFETY: see the `unsafe impl Send` justification directly above — the same
    // single-Worker-actor ownership invariant makes `&OpfsSqliteStore` safe to
    // share, vacuously, because no second thread can ever exist to share it.
    unsafe impl Sync for OpfsSqliteStore {}
}
