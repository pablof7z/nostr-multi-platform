//! NIP-09 (kind:5) deletion *application* policy for the OPFS-SQLite engine
//! (#1007 PR-3).
//!
//! Mirrors `nmp-store/src/lmdb/insert_kind5.rs`, minus the index families PR-3
//! does not yet own (FTS, LRU, interaction counters, freshness — later PRs). The
//! tombstone-row read/write half lives in [`crate::tombstones`] (mirroring the
//! LMDB split between `insert_kind5.rs` and `tombstones.rs`).
//!
//!   * Walk the kind:5's `e`-tags / `a`-tags; act on **self-deletes only**
//!     (foreign targets are silently skipped, matching the other backends).
//!   * Write an NMP tombstone (per-id for `e`-tags, coordinate for `a`-tags) so a
//!     later arrival of the deleted event is suppressed at insert time.
//!   * Remove any matching stored target (primary row + secondary tag rows +
//!     provenance) and append a `Deleted` ingest-log entry.
//!
//! All of this runs inside the single insert transaction supplied by the caller.

#![cfg(target_arch = "wasm32")]

use crate::conv::{self, EngineEvent};
use crate::error::SqliteWasmError;
use crate::ingest_log::DeleteReason;
use crate::outcome::{EventId, PubKey};
use crate::shim::SqliteConn;
use crate::store_impl::{blob32, exec_write, with_txn, SqlVal};
use crate::types::DeleteFilter;
use crate::{ingest_log, provenance, tombstones, OpfsSqliteStore};

/// Apply a kind:5 event's `e`-tag and `a`-tag self-deletes inside the insert
/// transaction (tombstones + target removal + `Deleted` log entries).
pub(crate) fn apply_kind5(
    conn: &SqliteConn,
    ev: &EngineEvent,
    source: &str,
    received_at_ms: u64,
) -> Result<(), SqliteWasmError> {
    // The caller (insert) only reaches kind:5 application after the structural
    // gate, so both decode; the `else` arms are defensive D6 no-panic fallbacks.
    let Some(kind5_id) = ev.id_bytes() else {
        return Err(SqliteWasmError::Encoding("kind:5 id not decodable".into()));
    };
    let Some(kind5_pubkey) = ev.pubkey_bytes() else {
        return Err(SqliteWasmError::Encoding(
            "kind:5 pubkey not decodable".into(),
        ));
    };
    let kind5_at = ev.created_at;

    for target_hex in ev.e_tags() {
        let Some(target_id) = conv::hex_to_bytes32(target_hex) else {
            continue;
        };
        // Self-delete check: an unknown (not-yet-stored) target is tombstoned for
        // future arrivals; a stored foreign target is skipped.
        match event_pubkey(conn, &target_id)? {
            Some(pk) if pk != kind5_pubkey => continue,
            stored => {
                tombstones::put_id(conn, &target_id, &kind5_id, &kind5_pubkey, kind5_at, source)?;
                if stored.is_some() {
                    remove_event(conn, &target_id)?;
                    ingest_log::append_deleted(
                        conn,
                        &kind5_id,
                        &target_id,
                        DeleteReason::Nip09,
                        received_at_ms,
                    )?;
                }
            }
        }
    }

    for addr in ev.a_tags() {
        let Some((tgt_kind, tgt_pk_hex, tgt_dtag)) = parse_coordinate(addr) else {
            continue;
        };
        if tgt_pk_hex != ev.pubkey {
            continue; // foreign target — skip.
        }
        tombstones::put_addr(
            conn,
            tgt_kind,
            &kind5_pubkey,
            tgt_dtag.as_bytes(),
            &kind5_id,
            &kind5_pubkey,
            kind5_at,
            source,
        )?;
        for removed in coordinate_targets(conn, tgt_kind, &kind5_pubkey, tgt_dtag, kind5_at)? {
            remove_event(conn, &removed)?;
            ingest_log::append_deleted(
                conn,
                &kind5_id,
                &removed,
                DeleteReason::Nip09,
                received_at_ms,
            )?;
        }
    }
    Ok(())
}

/// Remove an event everywhere it lives: secondary tag rows, provenance, the LRU
/// access row, then the primary row. (The primary delete also cascades
/// `event_tags` via the schema FK, but the explicit delete keeps removal correct
/// independent of the per-connection `foreign_keys` pragma.)
pub(crate) fn remove_event(conn: &SqliteConn, id: &EventId) -> Result<(), SqliteWasmError> {
    exec_write(
        conn,
        "DELETE FROM event_tags WHERE event_id = ?1",
        &[SqlVal::Blob(id)],
    )?;
    provenance::delete(conn, id)?;
    exec_write(
        conn,
        "DELETE FROM lru_access WHERE event_id = ?1",
        &[SqlVal::Blob(id)],
    )?;
    exec_write(
        conn,
        "DELETE FROM events WHERE id = ?1",
        &[SqlVal::Blob(id)],
    )
}

// ─── delete_by_filter (admin / GC bulk delete) ──────────────────────────────────

impl OpfsSqliteStore {
    /// Delete by an NMP-internal filter — for admin / GC purge paths.
    ///
    /// NOT a NIP-09 path (that flows through kind:5 `insert`). Removals + their
    /// ingest-log entries apply in ONE transaction. Mirrors the LMDB backend:
    /// `ByIds`/`ByAuthor`/`ByKindRange` emit `AdminPurge` log entries;
    /// `ByRelayOnly` (a retention removal of relay-exclusive events) emits none.
    /// Returns the number of primary rows removed.
    pub fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, SqliteWasmError> {
        let conn = self.db.borrow();
        with_txn(&conn, |c| match filter {
            DeleteFilter::ByIds(ids) => by_ids(c, &ids),
            DeleteFilter::ByAuthor(pk) => by_author(c, &pk),
            DeleteFilter::ByKindRange { lo, hi } => by_kind_range(c, lo, hi),
            DeleteFilter::ByRelayOnly(relay) => by_relay_only(c, &relay),
        })
    }
}

/// Remove `ids` that are actually present, appending an `AdminPurge` log entry
/// for each. The carrier id is the removed id itself (no kind:5 involved).
fn by_ids(conn: &SqliteConn, ids: &[EventId]) -> Result<usize, SqliteWasmError> {
    let mut n = 0usize;
    for id in ids {
        if !event_present(conn, id)? {
            continue;
        }
        remove_event(conn, id)?;
        ingest_log::append_deleted(conn, id, id, DeleteReason::AdminPurge, 0)?;
        n += 1;
    }
    Ok(n)
}

fn by_author(conn: &SqliteConn, pubkey: &PubKey) -> Result<usize, SqliteWasmError> {
    let ids = collect_ids(
        conn,
        "SELECT id FROM events WHERE pubkey = ?1",
        &[SqlVal::Blob(pubkey)],
    )?;
    remove_all_admin(conn, &ids)
}

fn by_kind_range(conn: &SqliteConn, lo: u32, hi: u32) -> Result<usize, SqliteWasmError> {
    let ids = collect_ids(
        conn,
        "SELECT id FROM events WHERE kind BETWEEN ?1 AND ?2",
        &[SqlVal::Int(i64::from(lo)), SqlVal::Int(i64::from(hi))],
    )?;
    remove_all_admin(conn, &ids)
}

/// Remove events seen on EXACTLY this relay (provenance has a single row and it
/// is this relay). A retention removal: no ingest-log entry (mirror of LMDB).
fn by_relay_only(conn: &SqliteConn, relay: &str) -> Result<usize, SqliteWasmError> {
    let ids = collect_ids(
        conn,
        "SELECT p.event_id FROM provenance p
         WHERE p.relay_url = ?1
           AND (SELECT COUNT(*) FROM provenance p2 WHERE p2.event_id = p.event_id) = 1",
        &[SqlVal::Text(relay)],
    )?;
    let mut n = 0usize;
    for id in &ids {
        remove_event(conn, id)?;
        n += 1;
    }
    Ok(n)
}

/// Remove every id with an `AdminPurge` log entry; returns the count.
fn remove_all_admin(conn: &SqliteConn, ids: &[EventId]) -> Result<usize, SqliteWasmError> {
    for id in ids {
        remove_event(conn, id)?;
        ingest_log::append_deleted(conn, id, id, DeleteReason::AdminPurge, 0)?;
    }
    Ok(ids.len())
}

fn event_present(conn: &SqliteConn, id: &EventId) -> Result<bool, SqliteWasmError> {
    let stmt = conn.prepare("SELECT 1 FROM events WHERE id = ?1")?;
    stmt.bind_blob(1, id)?;
    stmt.step()
}

/// Run an id-returning query and collect the 32-byte ids.
fn collect_ids(
    conn: &SqliteConn,
    sql: &str,
    params: &[SqlVal<'_>],
) -> Result<Vec<EventId>, SqliteWasmError> {
    let stmt = conn.prepare(sql)?;
    crate::store_impl::bind_params(&stmt, params)?;
    let mut out = Vec::new();
    while stmt.step()? {
        if let Some(id) = blob32(&stmt.column_blob(0)?) {
            out.push(id);
        }
    }
    Ok(out)
}

// ─── helpers ──────────────────────────────────────────────────────────────────

fn event_pubkey(conn: &SqliteConn, id: &EventId) -> Result<Option<PubKey>, SqliteWasmError> {
    let stmt = conn.prepare("SELECT pubkey FROM events WHERE id = ?1")?;
    stmt.bind_blob(1, id)?;
    if stmt.step()? {
        Ok(blob32(&stmt.column_blob(0)?))
    } else {
        Ok(None)
    }
}

/// Ids of stored events matching an `a`-tag coordinate with `created_at <=
/// kind5_at`. Addressable (param-replaceable) coordinates match on the d-tag;
/// regular-replaceable coordinates are unique per `(kind, pubkey)`.
fn coordinate_targets(
    conn: &SqliteConn,
    kind: u32,
    pubkey: &PubKey,
    d_tag: &str,
    kind5_at: u64,
) -> Result<Vec<EventId>, SqliteWasmError> {
    let stmt = if nmp_kinds::is_addressable(kind) {
        let s = conn.prepare(
            "SELECT id FROM events
             WHERE pubkey = ?1 AND kind = ?2 AND d_tag = ?3 AND created_at <= ?4",
        )?;
        s.bind_blob(1, pubkey)?;
        s.bind_int64(2, i64::from(kind))?;
        s.bind_blob(3, d_tag.as_bytes())?;
        s.bind_int64(4, kind5_at as i64)?;
        s
    } else {
        let s = conn.prepare(
            "SELECT id FROM events WHERE pubkey = ?1 AND kind = ?2 AND created_at <= ?3",
        )?;
        s.bind_blob(1, pubkey)?;
        s.bind_int64(2, i64::from(kind))?;
        s.bind_int64(3, kind5_at as i64)?;
        s
    };
    let mut ids = Vec::new();
    while stmt.step()? {
        if let Some(id) = blob32(&stmt.column_blob(0)?) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Parse an `a`-tag coordinate `"kind:pubkey-hex:d-tag"` into its parts.
fn parse_coordinate(addr: &str) -> Option<(u32, &str, &str)> {
    let mut parts = addr.splitn(3, ':');
    let kind = parts.next()?.parse::<u32>().ok()?;
    let pubkey = parts.next()?;
    let d_tag = parts.next()?;
    Some((kind, pubkey, d_tag))
}
