//! `nmp_meta` integer-counter helpers (#1007 PR-5).
//!
//! The `nmp_meta (key TEXT PRIMARY KEY, value INTEGER)` table holds small
//! monotonic counters/watermarks: the schema version (PR-3), the LRU access
//! sequence (`lru_seq`), and the ingest-log GC floor (`ingest_gc_floor`). These
//! helpers read/write/bump one counter; all run inside the caller's transaction
//! (or borrow) so they compose with the write paths that share a txn.

#![cfg(target_arch = "wasm32")]

use crate::error::SqliteWasmError;
use crate::shim::SqliteConn;
use crate::store_impl::{exec_write, SqlVal};

/// `nmp_meta` key holding the monotonic LRU access sequence counter.
pub(crate) const KEY_LRU_SEQ: &str = "lru_seq";

/// Read the counter at `key`, or `0` if it has never been written.
pub(crate) fn read_u64(conn: &SqliteConn, key: &str) -> Result<u64, SqliteWasmError> {
    let stmt = conn.prepare("SELECT value FROM nmp_meta WHERE key = ?1")?;
    stmt.bind_text(1, key)?;
    if stmt.step()? {
        Ok(stmt.column_int64(0)? as u64)
    } else {
        Ok(0)
    }
}

/// Set the counter at `key` to `value` (upsert).
pub(crate) fn write_u64(conn: &SqliteConn, key: &str, value: u64) -> Result<(), SqliteWasmError> {
    exec_write(
        conn,
        "INSERT INTO nmp_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        &[SqlVal::Text(key), SqlVal::Int(value as i64)],
    )
}

/// Increment the counter at `key` and return the new value. Allocates a strictly
/// increasing sequence (the LRU access stamp); must run inside a write txn so the
/// read-modify-write is atomic against the single Worker actor.
pub(crate) fn bump_u64(conn: &SqliteConn, key: &str) -> Result<u64, SqliteWasmError> {
    let next = read_u64(conn, key)?.saturating_add(1);
    write_u64(conn, key, next)?;
    Ok(next)
}
