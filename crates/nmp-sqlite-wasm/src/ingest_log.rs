//! Append-only ingest journal for the OPFS-SQLite engine (#1007 PR-3).
//!
//! Mirrors the LMDB ingest log (ADR-0058 §3-4): every accepted write appends one
//! entry inside the same transaction as the write (D4 — the seq is allocated and
//! the row committed atomically with the event). The `ingest_log.seq` column is
//! `INTEGER PRIMARY KEY AUTOINCREMENT`, so the seq is monotonic and never reused
//! even after a future trim — the relational equivalent of LMDB's `last_seq`
//! counter.
//!
//! Append-time trim / retention claims (ADR-0058 §6) are deliberately out of
//! scope for PR-3 (this PR establishes the monotonic seq + the three append ops);
//! the bounded-log trim lands with the GC PR. Each append uses a column-list
//! `INSERT` that omits the columns that do not apply, so SQLite stores SQL NULL
//! for them without an explicit null-bind.

#![cfg(target_arch = "wasm32")]

use crate::error::SqliteWasmError;
use crate::outcome::EventId;
use crate::shim::SqliteConn;
use crate::store_impl::{exec_write, SqlVal};

/// Append an `Inserted` entry (a fresh event store).
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
    )
}

/// Append a `Replaced` entry (a replaceable supersession; `target_id` is the
/// superseded event).
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
    )
}

/// Append a `Deleted` entry (a NIP-09 self-delete removed `target_id`; the
/// carrier is the kind:5 event).
pub(crate) fn append_deleted(
    conn: &SqliteConn,
    carrier_event_id: &EventId,
    target_id: &EventId,
    received_at_ms: u64,
) -> Result<(), SqliteWasmError> {
    exec_write(
        conn,
        "INSERT INTO ingest_log (op, event_id, target_id, received_at_ms)
         VALUES ('deleted', ?1, ?2, ?3)",
        &[
            SqlVal::Blob(carrier_event_id),
            SqlVal::Blob(target_id),
            SqlVal::Int(received_at_ms as i64),
        ],
    )
}
