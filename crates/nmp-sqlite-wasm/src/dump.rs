//! `dump` — stream all stored contents as JSONL (#1007 PR-5).
//!
//! Mirrors `nmp-store/src/lmdb/dump.rs` line-format and deterministic ordering
//! (events by id, tombstones by target id, domain rows by `(namespace, key)`) so
//! a `nmp dump` is comparable across backends. Three record shapes:
//!
//! ```text
//! {"type":"event","event":<NIP-01 object>,"received_at_ms":<u64>}
//! {"type":"tombstone","target_id":<hex>,"deleted_at":<u64>,"origin":<string>}
//! {"type":"domain","namespace":<string>,"key":<bytes>,"value":<bytes>}
//! ```
//!
//! Ordering note: the `events`/`tombstones` primary keys are the raw id bytes, so
//! `ORDER BY id` is byte order, which equals the hex-string order the other
//! backends sort by (hex is an order-preserving byte encoding).

#![cfg(target_arch = "wasm32")]

use std::io::Write;

use crate::conv;
use crate::error::SqliteWasmError;
use crate::store_impl::blob32;
use crate::types::DumpStats;
use crate::OpfsSqliteStore;

impl OpfsSqliteStore {
    /// Stream every stored event, tombstone, and domain row to `out` as JSONL.
    pub fn dump(&self, out: &mut impl Write) -> Result<DumpStats, SqliteWasmError> {
        let mut stats = DumpStats::default();
        let conn = self.db.borrow();

        // ── Events (ordered by id) ────────────────────────────────────────────
        let ev_stmt = conn.prepare("SELECT raw, received_at_ms FROM events ORDER BY id ASC")?;
        while ev_stmt.step()? {
            let event = conv::decode_blob(&ev_stmt.column_blob(0)?)?;
            let received_at_ms = ev_stmt.column_int64(1)? as u64;
            let line = serde_json::json!({
                "type": "event",
                "event": event,
                "received_at_ms": received_at_ms,
            })
            .to_string();
            write_line(out, &line, &mut stats.bytes_written)?;
            stats.events += 1;
        }

        // ── Tombstones (ordered by target id) ─────────────────────────────────
        let tb_stmt = conn
            .prepare("SELECT target_id, deleted_at, origin FROM tombstones ORDER BY target_id ASC")?;
        while tb_stmt.step()? {
            let target_id = blob32(&tb_stmt.column_blob(0)?)
                .ok_or_else(|| SqliteWasmError::Column("tombstone target_id not 32 bytes".into()))?;
            let deleted_at = tb_stmt.column_int64(1)? as u64;
            let origin = origin_name(tb_stmt.column_int64(2)?);
            let line = serde_json::json!({
                "type": "tombstone",
                "target_id": to_hex(&target_id),
                "deleted_at": deleted_at,
                "origin": origin,
            })
            .to_string();
            write_line(out, &line, &mut stats.bytes_written)?;
            stats.tombstones += 1;
        }

        // ── Domain rows (ordered by namespace, key) ───────────────────────────
        let dr_stmt = conn.prepare(
            "SELECT namespace, user_key, value FROM domain_data ORDER BY namespace ASC, user_key ASC",
        )?;
        while dr_stmt.step()? {
            let namespace = dr_stmt.column_text(0)?;
            let key = dr_stmt.column_blob(1)?;
            let value = dr_stmt.column_blob(2)?;
            let line = serde_json::json!({
                "type": "domain",
                "namespace": namespace,
                "key": key,
                "value": value,
            })
            .to_string();
            write_line(out, &line, &mut stats.bytes_written)?;
            stats.domain_rows += 1;
        }

        Ok(stats)
    }
}

/// Write `line` + newline, charging the byte total. A sink fault maps to `Exec`
/// (the wrapper turns it into `StoreError::Io`).
fn write_line(out: &mut impl Write, line: &str, total: &mut u64) -> Result<(), SqliteWasmError> {
    let bytes = format!("{line}\n").into_bytes();
    *total += bytes.len() as u64;
    out.write_all(&bytes)
        .map_err(|e| SqliteWasmError::Exec(format!("dump write: {e}")))
}

/// Stored tombstone `origin` code → the serde variant name the other backends
/// emit (`tombstones.rs`: 0 = Kind5, 1 = NIP40Expiry, 2 = AdminPurge).
fn origin_name(code: i64) -> &'static str {
    match code {
        0 => "Kind5",
        1 => "NIP40Expiry",
        _ => "AdminPurge",
    }
}

/// Lowercase-hex a 32-byte id.
fn to_hex(id: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(64);
    for b in id {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}
