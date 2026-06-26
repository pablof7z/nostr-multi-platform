//! Per-(event, relay) provenance for the OPFS-SQLite engine (#1007 PR-3).
//!
//! Mirrors the LMDB/Mem provenance LRU (`nmp-store/src/lmdb/provenance.rs`,
//! `mem/mod.rs:149-187`):
//!   * 32-entry cap ([`MAX_PROVENANCE_ENTRIES`]).
//!   * Existing relay → bump first/last seen.
//!   * Capacity full → overwrite the oldest non-primary entry.
//!   * Sort by `(first_seen_ms asc, relay_url asc)`; index 0 is `is_primary`.
//!
//! The LMDB backend serializes all of an event's entries into one JSON blob;
//! here each entry is a relational row (the relay-origin reverse index the LMDB
//! backend maintains as a separate sub-db is subsumed by `idx_prov_relay`). The
//! whole set is small (≤32), so an upsert rewrites the event's rows wholesale
//! inside the caller's transaction.

#![cfg(target_arch = "wasm32")]

use crate::error::SqliteWasmError;
use crate::outcome::EventId;
use crate::shim::SqliteConn;
use crate::store_impl::{exec_write, SqlVal};

/// Maximum provenance entries kept per event. Mirrors the other backends.
pub(crate) const MAX_PROVENANCE_ENTRIES: usize = 32;

struct Entry {
    relay_url: String,
    first_seen_ms: u64,
    last_seen_ms: u64,
}

/// Upsert the `(id, relay_url)` provenance entry and return the post-upsert
/// entry count (`InsertOutcome::*.sources_after`). Runs inside the caller's txn.
pub(crate) fn upsert(
    conn: &SqliteConn,
    id: &EventId,
    relay_url: &str,
    received_at_ms: u64,
) -> Result<u32, SqliteWasmError> {
    let mut entries = read_entries(conn, id)?;

    if let Some(e) = entries.iter_mut().find(|e| e.relay_url == relay_url) {
        e.first_seen_ms = e.first_seen_ms.min(received_at_ms);
        e.last_seen_ms = e.last_seen_ms.max(received_at_ms);
    } else if entries.len() >= MAX_PROVENANCE_ENTRIES {
        // Capacity full → overwrite the oldest non-primary entry (index 0 is the
        // primary after the sort, so skip it). `entries` is sorted by the read.
        if let Some(victim) = entries
            .iter_mut()
            .skip(1)
            .min_by_key(|e| e.last_seen_ms)
        {
            victim.relay_url = relay_url.to_string();
            victim.first_seen_ms = received_at_ms;
            victim.last_seen_ms = received_at_ms;
        }
    } else {
        entries.push(Entry {
            relay_url: relay_url.to_string(),
            first_seen_ms: received_at_ms,
            last_seen_ms: received_at_ms,
        });
    }

    sort_entries(&mut entries);
    write_entries(conn, id, &entries)?;
    Ok(entries.len() as u32)
}

/// Drop all provenance for an event id (used on replace / kind:5 removal).
pub(crate) fn delete(conn: &SqliteConn, id: &EventId) -> Result<(), SqliteWasmError> {
    exec_write(
        conn,
        "DELETE FROM provenance WHERE event_id = ?1",
        &[SqlVal::Blob(id)],
    )
}

fn read_entries(conn: &SqliteConn, id: &EventId) -> Result<Vec<Entry>, SqliteWasmError> {
    let stmt = conn.prepare(
        "SELECT relay_url, first_seen_ms, last_seen_ms FROM provenance WHERE event_id = ?1",
    )?;
    stmt.bind_blob(1, id)?;
    let mut out = Vec::new();
    while stmt.step()? {
        out.push(Entry {
            relay_url: stmt.column_text(0)?,
            first_seen_ms: stmt.column_int64(1)? as u64,
            last_seen_ms: stmt.column_int64(2)? as u64,
        });
    }
    sort_entries(&mut out);
    Ok(out)
}

fn write_entries(conn: &SqliteConn, id: &EventId, entries: &[Entry]) -> Result<(), SqliteWasmError> {
    exec_write(
        conn,
        "DELETE FROM provenance WHERE event_id = ?1",
        &[SqlVal::Blob(id)],
    )?;
    for (i, e) in entries.iter().enumerate() {
        exec_write(
            conn,
            "INSERT INTO provenance
                 (event_id, relay_url, first_seen_ms, last_seen_ms, is_primary)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                SqlVal::Blob(id),
                SqlVal::Text(&e.relay_url),
                SqlVal::Int(e.first_seen_ms as i64),
                SqlVal::Int(e.last_seen_ms as i64),
                SqlVal::Int(i64::from(i == 0)),
            ],
        )?;
    }
    Ok(())
}

fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        a.first_seen_ms
            .cmp(&b.first_seen_ms)
            .then_with(|| a.relay_url.cmp(&b.relay_url))
    });
}
