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
use crate::outcome::{EventId, ProvenanceRow};
use crate::shim::SqliteConn;
use crate::store_impl::{exec_write, SqlVal};
use crate::OpfsSqliteStore;

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
        if let Some(victim) = entries.iter_mut().skip(1).min_by_key(|e| e.last_seen_ms) {
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

fn write_entries(
    conn: &SqliteConn,
    id: &EventId,
    entries: &[Entry],
) -> Result<(), SqliteWasmError> {
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

// ─── Read side (#1007 PR-4) ────────────────────────────────────────────────────

impl OpfsSqliteStore {
    /// All provenance rows for `id`, sorted `(first_seen_ms asc, relay_url asc)`
    /// so index 0 is the deterministic primary (mirror of the `is_primary` write
    /// invariant). Empty when the event has no provenance (absent / removed).
    pub fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceRow>, SqliteWasmError> {
        let conn = self.conn().borrow();
        let stmt = conn.prepare(
            "SELECT relay_url, first_seen_ms, last_seen_ms, is_primary FROM provenance \
             WHERE event_id = ?1 ORDER BY first_seen_ms ASC, relay_url ASC",
        )?;
        stmt.bind_blob(1, id)?;
        let mut out = Vec::new();
        while stmt.step()? {
            out.push(ProvenanceRow {
                relay_url: stmt.column_text(0)?,
                first_seen_ms: stmt.column_int64(1)? as u64,
                last_seen_ms: stmt.column_int64(2)? as u64,
                is_primary: stmt.column_int64(3)? != 0,
            });
        }
        Ok(out)
    }

    /// V-52 — ids of events whose provenance includes `relay_url`.
    ///
    /// Index-served by `idx_prov_relay` (`relay_url, event_id`), joined to
    /// `events` so only events **still present** in the store appear (every
    /// removal path prunes provenance, but the join makes the contract explicit
    /// and immune to a stray orphan row).
    pub fn list_events_seen_on(&self, relay_url: &str) -> Result<Vec<EventId>, SqliteWasmError> {
        let conn = self.conn().borrow();
        let stmt = conn.prepare(
            "SELECT p.event_id FROM provenance p \
             JOIN events e ON e.id = p.event_id \
             WHERE p.relay_url = ?1",
        )?;
        stmt.bind_text(1, relay_url)?;
        let mut out = Vec::new();
        while stmt.step()? {
            let bytes = stmt.column_blob(0)?;
            match <EventId>::try_from(bytes.as_slice()) {
                Ok(id) => out.push(id),
                Err(_) => {
                    return Err(SqliteWasmError::Column(
                        "provenance event_id not 32 bytes".into(),
                    ))
                }
            }
        }
        Ok(out)
    }

    /// #1518 — the distinct kinds `relay_url` has served, ascending.
    ///
    /// Derived from the per-event provenance × `events.kind` projection (the
    /// reverse index the LMDB backend keeps as a dedicated sub-db is subsumed by
    /// `idx_prov_relay` + the join). Privacy-gated kinds are filtered here
    /// because SQLite derives relay-kind coverage from provenance rows instead
    /// of maintaining a separate relay-kind table.
    pub fn relay_kind_coverage(&self, relay_url: &str) -> Result<Vec<u32>, SqliteWasmError> {
        let conn = self.conn().borrow();
        let stmt = conn.prepare(
            "SELECT DISTINCT e.kind FROM provenance p \
             JOIN events e ON e.id = p.event_id \
             WHERE p.relay_url = ?1 ORDER BY e.kind ASC",
        )?;
        stmt.bind_text(1, relay_url)?;
        let mut out = Vec::new();
        while stmt.step()? {
            let kind = stmt.column_int64(0)? as u32;
            if !nmp_kinds::is_private_relay_provenance_kind(kind) {
                out.push(kind);
            }
        }
        Ok(out)
    }

    /// #1518 — how many distinct events of `kind` `relay_url` has served.
    ///
    /// `(event_id, relay_url)` is the provenance primary key, so each event is
    /// counted once. Same projection as [`Self::relay_kind_coverage`].
    pub fn relay_kind_count(&self, relay_url: &str, kind: u32) -> Result<u64, SqliteWasmError> {
        if nmp_kinds::is_private_relay_provenance_kind(kind) {
            return Ok(0);
        }
        let conn = self.conn().borrow();
        let stmt = conn.prepare(
            "SELECT COUNT(*) FROM provenance p \
             JOIN events e ON e.id = p.event_id \
             WHERE p.relay_url = ?1 AND e.kind = ?2",
        )?;
        stmt.bind_text(1, relay_url)?;
        stmt.bind_int64(2, i64::from(kind))?;
        if stmt.step()? {
            Ok(stmt.column_int64(0)? as u64)
        } else {
            Ok(0)
        }
    }
}
