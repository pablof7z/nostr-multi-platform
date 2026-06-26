//! SQLite read/write/trim paths for the ingest journal (#1007 PR-5).
//!
//! The append helpers (`insert`/`delete` call these), the append-time trim
//! (BLOCKING 4), and the four inherent read/claim methods
//! (`latest_ingest_seq`, `oldest_available_seq`, `scan_log_since_seq`,
//! `replace_log_retention_claims`). Pure types live in [`crate::ingest_log`].

#![cfg(target_arch = "wasm32")]

use crate::conv;
use crate::error::SqliteWasmError;
use crate::ingest_log::{
    DeleteReason, LogOp, LogRetentionClaim, PullGap, PullPage, ScanLogResult, StoreLogEntry,
    DEFAULT_LOG_MAX_ENTRIES, KEY_INGEST_GC_FLOOR,
};
use crate::meta;
use crate::outcome::EventId;
use crate::shim::{SqliteConn, SqliteStmt};
use crate::store_impl::{blob32, exec_write, with_txn, SqlVal};
use crate::OpfsSqliteStore;

/// Append an `Inserted` entry, then trim the log inside the same txn.
pub(crate) fn append_inserted(
    conn: &SqliteConn,
    event_id: &EventId,
    raw_event: &[u8],
    source_relay: &str,
    received_at_ms: u64,
) -> Result<(), SqliteWasmError> {
    exec_write(
        conn,
        "INSERT INTO ingest_log (op, event_id, raw_event, source_relay, received_at_ms)
         VALUES ('inserted', ?1, ?2, ?3, ?4)",
        &[
            SqlVal::Blob(event_id),
            SqlVal::Blob(raw_event),
            SqlVal::Text(source_relay),
            SqlVal::Int(received_at_ms as i64),
        ],
    )?;
    trim_in_txn(conn)
}

/// Append a `Replaced` entry (`target_id` = the superseded event), then trim.
pub(crate) fn append_replaced(
    conn: &SqliteConn,
    new_event_id: &EventId,
    replaced_id: &EventId,
    raw_event: &[u8],
    source_relay: &str,
    received_at_ms: u64,
) -> Result<(), SqliteWasmError> {
    exec_write(
        conn,
        "INSERT INTO ingest_log (op, event_id, target_id, raw_event, source_relay, received_at_ms)
         VALUES ('replaced', ?1, ?2, ?3, ?4, ?5)",
        &[
            SqlVal::Blob(new_event_id),
            SqlVal::Blob(replaced_id),
            SqlVal::Blob(raw_event),
            SqlVal::Text(source_relay),
            SqlVal::Int(received_at_ms as i64),
        ],
    )?;
    trim_in_txn(conn)
}

/// Append a `Deleted` entry (the carrier is the deleting event), then trim.
pub(crate) fn append_deleted(
    conn: &SqliteConn,
    carrier_event_id: &EventId,
    target_id: &EventId,
    reason: DeleteReason,
    received_at_ms: u64,
) -> Result<(), SqliteWasmError> {
    exec_write(
        conn,
        "INSERT INTO ingest_log (op, event_id, target_id, reason, received_at_ms)
         VALUES ('deleted', ?1, ?2, ?3, ?4)",
        &[
            SqlVal::Blob(carrier_event_id),
            SqlVal::Blob(target_id),
            SqlVal::Text(reason.as_db_str()),
            SqlVal::Int(received_at_ms as i64),
        ],
    )?;
    trim_in_txn(conn)
}

impl OpfsSqliteStore {
    /// The highest seq allocated so far (0 if the log is empty). The
    /// AUTOINCREMENT high-water mark, so it survives a trim of the named row.
    pub fn latest_ingest_seq(&self) -> Result<u64, SqliteWasmError> {
        let conn = self.db.borrow();
        latest_seq(&conn)
    }

    /// The lowest seq still physically present, or `None` if the log is empty.
    pub fn oldest_available_seq(&self) -> Result<Option<u64>, SqliteWasmError> {
        let conn = self.db.borrow();
        let stmt = conn.prepare("SELECT MIN(seq) FROM ingest_log")?;
        if stmt.step()? {
            // MIN over an empty table is SQL NULL (reads back as 0); a guard row
            // tells "empty" from "lowest present seq is 0" apart.
            let any = conn.prepare("SELECT 1 FROM ingest_log LIMIT 1")?;
            if any.step()? {
                return Ok(Some(stmt.column_int64(0)? as u64));
            }
        }
        Ok(None)
    }

    /// Scan entries with `seq > after_seq`, ascending, up to `limit`. Returns an
    /// explicit [`ScanLogResult::Gap`] when `after_seq` is behind the GC floor —
    /// never a silent skip.
    pub fn scan_log_since_seq(
        &self,
        after_seq: u64,
        limit: usize,
    ) -> Result<ScanLogResult, SqliteWasmError> {
        let conn = self.db.borrow();
        let latest = latest_seq(&conn)?;
        let gc_floor = meta::read_u64(&conn, KEY_INGEST_GC_FLOOR)?;

        if gc_floor > 0 && after_seq < gc_floor {
            return Ok(ScanLogResult::Gap(PullGap {
                requested_after_seq: after_seq,
                first_available_seq: gc_floor + 1,
            }));
        }
        // Guard u64::MAX so `after_seq + 1` never overflows before any FFI.
        if after_seq.checked_add(1).is_none() {
            return Ok(ScanLogResult::Page(PullPage {
                entries: vec![],
                next_after_seq: after_seq,
                latest_seq: latest,
                has_more: false,
            }));
        }

        let stmt = conn.prepare(
            "SELECT seq, op, event_id, target_id, reason, raw_event, source_relay, received_at_ms
             FROM ingest_log WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2",
        )?;
        stmt.bind_int64(1, after_seq as i64)?;
        stmt.bind_int64(2, limit as i64)?;
        let mut entries: Vec<StoreLogEntry> = Vec::new();
        while stmt.step()? {
            entries.push(decode_row(&stmt)?);
        }
        let next_after_seq = entries.last().map(|e| e.seq).unwrap_or(after_seq);
        let has_more = next_after_seq < latest;
        Ok(ScanLogResult::Page(PullPage {
            entries,
            next_after_seq,
            latest_seq: latest,
            has_more,
        }))
    }

    /// Replace the whole volatile `Protected`-cursor retention-claim set. The
    /// next append-time trim reads it to pick the protected floor.
    pub fn replace_log_retention_claims(
        &self,
        claims: &[LogRetentionClaim],
    ) -> Result<(), SqliteWasmError> {
        let conn = self.db.borrow();
        with_txn(&conn, |c| {
            exec_write(c, "DELETE FROM log_retention_claims", &[])?;
            for claim in claims {
                exec_write(
                    c,
                    "INSERT INTO log_retention_claims (after_seq, max_lag_entries)
                     VALUES (?1, ?2)",
                    &[
                        SqlVal::Int(claim.after_seq as i64),
                        SqlVal::Int(claim.max_lag_entries as i64),
                    ],
                )?;
            }
            Ok(())
        })
    }
}

/// The current high-water seq (AUTOINCREMENT counter), 0 when never used. Read
/// from `sqlite_sequence` so it is the allocated max even after the row it named
/// has been trimmed away (mirrors LMDB's persisted `last_seq`).
fn latest_seq(conn: &SqliteConn) -> Result<u64, SqliteWasmError> {
    let stmt = conn.prepare("SELECT seq FROM sqlite_sequence WHERE name = 'ingest_log'")?;
    if stmt.step()? {
        Ok(stmt.column_int64(0)? as u64)
    } else {
        Ok(0)
    }
}

/// BLOCKING 4 — trim inside the caller's append txn so the log is never
/// unbounded between GC passes. Advances `gc_floor` to the normal retention
/// floor, capped below the slowest still-eligible protected-cursor claim, and
/// deletes the rows below it. Eligibility is recomputed against the current
/// `latest_seq` (a stuck cursor cannot pin the log unbounded — D5).
fn trim_in_txn(conn: &SqliteConn) -> Result<(), SqliteWasmError> {
    let latest = latest_seq(conn)?;
    let floor = meta::read_u64(conn, KEY_INGEST_GC_FLOOR)?;
    let normal_floor = latest.saturating_sub(DEFAULT_LOG_MAX_ENTRIES);
    let protected_floor = eligible_protected_floor(conn, latest)?;

    let target_floor = match protected_floor {
        Some(p) => normal_floor.min(p),
        None => normal_floor,
    };
    let new_floor = floor.max(target_floor);
    if new_floor <= floor {
        return Ok(());
    }

    exec_write(
        conn,
        "DELETE FROM ingest_log WHERE seq <= ?1",
        &[SqlVal::Int(new_floor as i64)],
    )?;
    // Always advance gc_floor (even if no physical rows fell in range) so the gap
    // contract holds: a scan below it returns first_available = floor + 1.
    meta::write_u64(conn, KEY_INGEST_GC_FLOOR, new_floor)
}

/// Min `after_seq` among claims still within their lag bound at `latest`, or
/// `None` when none are eligible (a stuck cursor self-evicts here — D5).
fn eligible_protected_floor(
    conn: &SqliteConn,
    latest: u64,
) -> Result<Option<u64>, SqliteWasmError> {
    let stmt = conn.prepare(
        "SELECT MIN(after_seq) FROM log_retention_claims
         WHERE (?1 - after_seq) <= max_lag_entries",
    )?;
    stmt.bind_int64(1, latest as i64)?;
    if stmt.step()? {
        // MIN over no eligible rows is NULL (reads back 0); a guard distinguishes.
        let any = conn.prepare(
            "SELECT 1 FROM log_retention_claims WHERE (?1 - after_seq) <= max_lag_entries LIMIT 1",
        )?;
        any.bind_int64(1, latest as i64)?;
        if any.step()? {
            return Ok(Some(stmt.column_int64(0)? as u64));
        }
    }
    Ok(None)
}

/// Decode one `ingest_log` result row into a [`StoreLogEntry`].
fn decode_row(stmt: &SqliteStmt<'_>) -> Result<StoreLogEntry, SqliteWasmError> {
    let seq = stmt.column_int64(0)? as u64;
    let op_str = stmt.column_text(1)?;
    let event_id = blob32(&stmt.column_blob(2)?)
        .ok_or_else(|| SqliteWasmError::Column("ingest_log event_id not 32 bytes".into()))?;
    let target_id = blob32(&stmt.column_blob(3)?);
    let reason_str = stmt.column_text(4)?;
    let raw_blob = stmt.column_blob(5)?;
    let source = stmt.column_text(6)?;
    let received_at_ms = stmt.column_int64(7)? as u64;

    let op = match op_str.as_str() {
        "inserted" => LogOp::Inserted,
        "replaced" => LogOp::Replaced {
            replaced_id: target_id
                .ok_or_else(|| SqliteWasmError::Column("replaced entry missing target_id".into()))?,
        },
        "deleted" => LogOp::Deleted {
            target_id: target_id
                .ok_or_else(|| SqliteWasmError::Column("deleted entry missing target_id".into()))?,
            reason: DeleteReason::from_db_str(&reason_str).ok_or_else(|| {
                SqliteWasmError::Column(format!("unknown delete reason '{reason_str}'"))
            })?,
        },
        other => {
            return Err(SqliteWasmError::Column(format!(
                "unknown ingest_log op '{other}'"
            )))
        }
    };

    let raw_event = if raw_blob.is_empty() {
        None
    } else {
        Some(conv::decode_blob(&raw_blob)?)
    };
    let source_relay = if source.is_empty() { None } else { Some(source) };

    Ok(StoreLogEntry {
        seq,
        op,
        event_id,
        raw_event,
        source_relay,
        received_at_ms,
    })
}
