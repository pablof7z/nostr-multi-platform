//! Tombstone storage for the OPFS-SQLite engine (#1007 PR-3).
//!
//! The read/write half of NIP-09 deletion — the per-id (`tombstones`) and
//! addressable-coordinate (`addr_tombstones`) rows that suppress a later arrival
//! of a deleted event. Mirrors `nmp-store/src/lmdb/tombstones.rs`; the kind:5
//! *application* policy that produces these rows lives in [`crate::delete`]
//! (mirroring the LMDB split between `tombstones.rs` and `insert_kind5.rs`).
//!
//! PR-3 only ever writes `Kind5`-origin tombstones; the `NIP40Expiry` /
//! `AdminPurge` origins exist for the GC reaper / admin paths that land later,
//! so the `origin` column is decoded into the full [`TombstoneOrigin`] set but
//! only `Kind5` is currently written.

#![cfg(target_arch = "wasm32")]

use crate::error::SqliteWasmError;
use crate::outcome::{EventId, PubKey, TombstoneOrigin, TombstoneRow};
use crate::shim::{SqliteConn, SqliteStmt};
use crate::store_impl::{blob32, exec_write, SqlVal};
use crate::OpfsSqliteStore;

// Stored `origin` codes.
const ORIGIN_KIND5: i64 = 0;
const ORIGIN_NIP40: i64 = 1;
const ORIGIN_ADMIN: i64 = 2;

// ─── Read side (the EventStore wrapper's tombstones_for / list_tombstones) ───────

impl OpfsSqliteStore {
    /// The per-id tombstone(s) referencing `target` — at most one row in this
    /// engine (the `tombstones` primary key is `target_id`). Empty when absent.
    pub fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, SqliteWasmError> {
        let conn = self.conn().borrow();
        let stmt = conn.prepare(
            "SELECT target_id, kind5_event_id, deleter_pubkey, deleted_at, origin, source \
             FROM tombstones WHERE target_id = ?1",
        )?;
        stmt.bind_blob(1, target)?;
        let mut out = Vec::new();
        while stmt.step()? {
            out.push(decode_tombstone_row(&stmt)?);
        }
        Ok(out)
    }

    /// Every per-id tombstone row, ordered by `target_id` (used by `nmp dump` and
    /// the wrapper's `list_tombstones`).
    pub fn list_tombstones(&self) -> Result<Vec<TombstoneRow>, SqliteWasmError> {
        let conn = self.conn().borrow();
        let stmt = conn.prepare(
            "SELECT target_id, kind5_event_id, deleter_pubkey, deleted_at, origin, source \
             FROM tombstones ORDER BY target_id ASC",
        )?;
        let mut out = Vec::new();
        while stmt.step()? {
            out.push(decode_tombstone_row(&stmt)?);
        }
        Ok(out)
    }
}

/// Decode a `SELECT target_id, kind5_event_id, deleter_pubkey, deleted_at,
/// origin, source` row into a [`TombstoneRow`]. The `source` column is `None`
/// (empty `sources`) when NULL/empty.
fn decode_tombstone_row(stmt: &SqliteStmt<'_>) -> Result<TombstoneRow, SqliteWasmError> {
    let target_id = blob32(&stmt.column_blob(0)?)
        .ok_or_else(|| SqliteWasmError::Column("tombstone target_id not 32 bytes".into()))?;
    let kind5_event_id = blob32(&stmt.column_blob(1)?);
    let deleter_pubkey = blob32(&stmt.column_blob(2)?);
    let deleted_at = stmt.column_int64(3)? as u64;
    let origin = origin_from_code(stmt.column_int64(4)?);
    let source = stmt.column_text(5)?;
    let sources = if source.is_empty() {
        Vec::new()
    } else {
        vec![source]
    };
    Ok(TombstoneRow {
        target_id,
        kind5_event_id,
        deleter_pubkey,
        deleted_at,
        sources,
        origin,
    })
}

fn origin_from_code(code: i64) -> TombstoneOrigin {
    match code {
        ORIGIN_KIND5 => TombstoneOrigin::Kind5,
        ORIGIN_NIP40 => TombstoneOrigin::NIP40Expiry,
        ORIGIN_ADMIN => TombstoneOrigin::AdminPurge,
        // Unknown/future code — fail safe to the most conservative origin.
        _ => TombstoneOrigin::AdminPurge,
    }
}

/// `kind(BE4) || pubkey(32) || d-tag bytes` — the addressable-coordinate key
/// (mirror of LMDB `make_coordinate_index_key`).
pub(crate) fn coord_key(kind: u32, pubkey: &PubKey, d_tag: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + d_tag.len());
    key.extend_from_slice(&kind.to_be_bytes());
    key.extend_from_slice(pubkey);
    key.extend_from_slice(d_tag);
    key
}

/// A per-id tombstone row, as read for the insert-time suppression check.
pub(crate) struct PerIdTombstone {
    /// The kind:5 event that caused this tombstone (PR-3 only writes Kind5 rows).
    pub kind5_event_id: Option<EventId>,
    /// The deleter pubkey (always present for a Kind5 tombstone).
    pub deleter_pubkey: Option<PubKey>,
    /// What produced the tombstone.
    pub origin: TombstoneOrigin,
}

/// Read the per-id tombstone for `target`, if any.
pub(crate) fn get(
    conn: &SqliteConn,
    target: &EventId,
) -> Result<Option<PerIdTombstone>, SqliteWasmError> {
    let stmt = conn.prepare(
        "SELECT kind5_event_id, deleter_pubkey, origin FROM tombstones WHERE target_id = ?1",
    )?;
    stmt.bind_blob(1, target)?;
    if stmt.step()? {
        Ok(Some(PerIdTombstone {
            kind5_event_id: blob32(&stmt.column_blob(0)?),
            deleter_pubkey: blob32(&stmt.column_blob(1)?),
            origin: origin_from_code(stmt.column_int64(2)?),
        }))
    } else {
        Ok(None)
    }
}

/// Drop the per-id tombstone for `target` (foreign pre-tombstone supersession).
pub(crate) fn delete(conn: &SqliteConn, target: &EventId) -> Result<(), SqliteWasmError> {
    exec_write(
        conn,
        "DELETE FROM tombstones WHERE target_id = ?1",
        &[SqlVal::Blob(target)],
    )
}

/// The `(deleted_at, kind5_event_id)` of the coordinate tombstone for
/// `(kind, pubkey, d_tag)`, if any — used by insert to suppress an addressable
/// older than its deletion.
pub(crate) fn addr_deleted_at(
    conn: &SqliteConn,
    kind: u32,
    pubkey: &PubKey,
    d_tag: &[u8],
) -> Result<Option<(u64, Option<EventId>)>, SqliteWasmError> {
    let coord = coord_key(kind, pubkey, d_tag);
    let stmt =
        conn.prepare("SELECT deleted_at, kind5_event_id FROM addr_tombstones WHERE coord = ?1")?;
    stmt.bind_blob(1, &coord)?;
    if stmt.step()? {
        Ok(Some((
            stmt.column_int64(0)? as u64,
            blob32(&stmt.column_blob(1)?),
        )))
    } else {
        Ok(None)
    }
}

/// Write/merge the per-id tombstone for a kind:5 `e`-tag self-delete. Max-merge:
/// a later (higher `deleted_at`) kind:5 wins the metadata.
pub(crate) fn put_id(
    conn: &SqliteConn,
    target_id: &EventId,
    kind5_id: &EventId,
    deleter_pubkey: &PubKey,
    deleted_at: u64,
    source: &str,
) -> Result<(), SqliteWasmError> {
    exec_write(
        conn,
        "INSERT INTO tombstones
             (target_id, kind5_event_id, deleter_pubkey, deleted_at, origin, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(target_id) DO UPDATE SET
             kind5_event_id = CASE WHEN excluded.deleted_at >= deleted_at
                                   THEN excluded.kind5_event_id ELSE kind5_event_id END,
             deleter_pubkey = CASE WHEN excluded.deleted_at >= deleted_at
                                   THEN excluded.deleter_pubkey ELSE deleter_pubkey END,
             source         = CASE WHEN excluded.deleted_at >= deleted_at
                                   THEN excluded.source ELSE source END,
             deleted_at     = MAX(deleted_at, excluded.deleted_at)",
        &[
            SqlVal::Blob(target_id),
            SqlVal::Blob(kind5_id),
            SqlVal::Blob(deleter_pubkey),
            SqlVal::Int(deleted_at as i64),
            SqlVal::Int(ORIGIN_KIND5),
            SqlVal::Text(source),
        ],
    )
}

/// Write/merge the coordinate tombstone for a kind:5 `a`-tag self-delete.
#[allow(clippy::too_many_arguments)]
pub(crate) fn put_addr(
    conn: &SqliteConn,
    kind: u32,
    coord_pubkey: &PubKey,
    d_tag: &[u8],
    kind5_id: &EventId,
    deleter_pubkey: &PubKey,
    deleted_at: u64,
    source: &str,
) -> Result<(), SqliteWasmError> {
    let coord = coord_key(kind, coord_pubkey, d_tag);
    exec_write(
        conn,
        "INSERT INTO addr_tombstones
             (coord, kind5_event_id, deleter_pubkey, deleted_at, origin, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(coord) DO UPDATE SET
             kind5_event_id = CASE WHEN excluded.deleted_at >= deleted_at
                                   THEN excluded.kind5_event_id ELSE kind5_event_id END,
             deleter_pubkey = CASE WHEN excluded.deleted_at >= deleted_at
                                   THEN excluded.deleter_pubkey ELSE deleter_pubkey END,
             source         = CASE WHEN excluded.deleted_at >= deleted_at
                                   THEN excluded.source ELSE source END,
             deleted_at     = MAX(deleted_at, excluded.deleted_at)",
        &[
            SqlVal::Blob(&coord),
            SqlVal::Blob(kind5_id),
            SqlVal::Blob(deleter_pubkey),
            SqlVal::Int(deleted_at as i64),
            SqlVal::Int(ORIGIN_KIND5),
            SqlVal::Text(source),
        ],
    )
}
