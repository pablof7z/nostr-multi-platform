//! GC Phase 3 — tombstone purge for the OPFS-SQLite engine (#1007 PR-5).
//!
//! Per-id (`tombstones`) and address (`addr_tombstones`) rows older than 90 days
//! are dropped, throttled to once per hour using the caller-supplied `now_secs`
//! (D7 — the store never reads the clock itself; the last-purge time is persisted
//! in `nmp_meta`). Split out of `gc.rs` to keep that file under the LOC cap,
//! mirroring the LMDB backend's `gc_tombstones.rs`.

#![cfg(target_arch = "wasm32")]

use crate::error::SqliteWasmError;
use crate::meta;
use crate::shim::SqliteConn;
use crate::store_impl::{exec_write, with_txn, SqlVal};
use crate::types::GcReport;

/// Per-id / address tombstones older than this are purged. 90 days.
const TOMBSTONE_MAX_AGE_SECS: u64 = 90 * 24 * 60 * 60;
/// Phase 3 runs at most once per hour (mirror of LMDB V-117).
const GC_TOMBSTONE_PURGE_INTERVAL_SECS: u64 = 3_600;
/// `nmp_meta` key recording when Phase 3 last ran (throttle, D7-safe).
const KEY_LAST_TOMBSTONE_PURGE: &str = "gc_last_tombstone_purge_secs";

/// Purge aged tombstones (throttled). Updates `report.tombstones_purged` /
/// `report.addr_tombstones_purged`.
pub(crate) fn purge_tombstones(
    conn: &SqliteConn,
    now_secs: u64,
    report: &mut GcReport,
) -> Result<(), SqliteWasmError> {
    let last = meta::read_u64(conn, KEY_LAST_TOMBSTONE_PURGE)?;
    if now_secs.saturating_sub(last) < GC_TOMBSTONE_PURGE_INTERVAL_SECS {
        return Ok(()); // throttled — at most once per hour
    }
    let threshold = now_secs.saturating_sub(TOMBSTONE_MAX_AGE_SECS);
    with_txn(conn, |c| {
        report.tombstones_purged += purge(c, "tombstones", threshold)?;
        report.addr_tombstones_purged += purge(c, "addr_tombstones", threshold)?;
        meta::write_u64(c, KEY_LAST_TOMBSTONE_PURGE, now_secs)
    })
}

/// Delete rows with `deleted_at < threshold` from `table`, returning the count.
/// `table` is a fixed module-internal literal — never user input.
fn purge(conn: &SqliteConn, table: &str, threshold: u64) -> Result<usize, SqliteWasmError> {
    let count = {
        let stmt = conn.prepare(&format!(
            "SELECT COUNT(*) FROM {table} WHERE deleted_at < ?1"
        ))?;
        stmt.bind_int64(1, threshold as i64)?;
        if stmt.step()? {
            stmt.column_int64(0)? as usize
        } else {
            0
        }
    };
    if count > 0 {
        exec_write(
            conn,
            &format!("DELETE FROM {table} WHERE deleted_at < ?1"),
            &[SqlVal::Int(threshold as i64)],
        )?;
    }
    Ok(count)
}
