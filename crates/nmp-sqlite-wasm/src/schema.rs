//! SQLite DDL + migration for the OPFS-SQLite engine (#1007 PR-3).
//!
//! ## Why these tables
//!
//! The native LMDB engine ([`nmp-nostr-lmdb`]) has no secondary indexes, so it
//! materializes each access path as a separate key-value sub-db whose key is the
//! concatenated dimensions (`ci`, `tci`, `aci`, `akci`, `atci`, `kci`, `ktci` —
//! see `crates/nmp-nostr-lmdb/src/store/lmdb/setup.rs` and `index.rs`). SQLite
//! **has** B-tree secondary indexes, so the faithful translation is a normalized
//! schema + one `CREATE INDEX` per access path (Article VIII — trust the
//! framework; do not hand-roll LMDB-style duplicate index tables). Every LMDB
//! scan path therefore has an index-served SQL equivalent:
//!
//! | LMDB sub-db | dimensions                          | SQL index (this file)        |
//! |-------------|-------------------------------------|------------------------------|
//! | `ci`        | created_at, id                      | `idx_events_ci`              |
//! | `aci`       | author, created_at, id              | `idx_events_aci`             |
//! | `akci`      | author, kind, created_at, id        | `idx_events_akci`            |
//! | `kci`       | kind, created_at, id                | `idx_events_kci`             |
//! | `tci`       | tag, created_at, id                 | `idx_tags_tci`               |
//! | `atci`      | author, tag, created_at, id         | `idx_tags_atci`              |
//! | `ktci`      | kind, tag, created_at, id           | `idx_tags_ktci`              |
//!
//! Newest-first ordering: LMDB stores a reversed `created_at` in the key; SQLite
//! expresses the same with a `created_at DESC, id ASC` index, matching the
//! `(created_at desc, id asc)` global order the `EventStore` trait specifies.
//!
//! The single `event_tags` table (one row per single-letter tag occurrence,
//! carrying the redundant `pubkey` / `kind` / `created_at` so a composite index
//! covers the scan) subsumes the three LMDB tag sub-dbs. The relay-origin and
//! relay×kind LMDB sub-dbs are likewise subsumed: with relational `provenance`
//! and a JOIN to `events`, `list_events_seen_on` and relay×kind coverage are
//! plain indexed queries — no separate reverse-index table.
//!
//! Tombstones (`tombstones`, `addr_tombstones`), provenance, and the monotonic
//! ingest-log seq complete the set. `nmp_meta` carries the schema version for
//! migrations.

/// Current on-disk schema version. Bump + add a migration branch in
/// [`ensure_schema`] when the DDL changes.
pub const SCHEMA_VERSION: i64 = 1;

/// The full DDL, applied idempotently on every open (`IF NOT EXISTS`).
///
/// Kept as a target-agnostic `const` so the wasm apply path and any native
/// tooling share one source of truth.
pub const SCHEMA_SQL: &str = r#"
-- Schema-version / migration bookkeeping.
CREATE TABLE IF NOT EXISTS nmp_meta (
    key   TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);

-- Primary event store. `id` / `pubkey` are the raw 32-byte values (compact key
-- + fast equality); `raw` is the canonical NIP-01 JSON blob (see `conv`).
CREATE TABLE IF NOT EXISTS events (
    id             BLOB PRIMARY KEY,
    pubkey         BLOB NOT NULL,
    kind           INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    d_tag          BLOB,
    expires_at     INTEGER,
    raw            BLOB NOT NULL,
    received_at_ms INTEGER NOT NULL
) WITHOUT ROWID;

-- ci / aci / akci / kci access paths (newest-first).
CREATE INDEX IF NOT EXISTS idx_events_ci   ON events (created_at DESC, id ASC);
CREATE INDEX IF NOT EXISTS idx_events_aci  ON events (pubkey, created_at DESC, id ASC);
CREATE INDEX IF NOT EXISTS idx_events_akci ON events (pubkey, kind, created_at DESC, id ASC);
CREATE INDEX IF NOT EXISTS idx_events_kci  ON events (kind, created_at DESC, id ASC);
-- Parameterized-replaceable lookup (kind + pubkey + d-tag, newest wins).
CREATE INDEX IF NOT EXISTS idx_events_dtag ON events (pubkey, kind, d_tag, created_at DESC);
-- Cross-author addressable scan (kind + d-tag, newest-first) — the
-- `idx_kind_dtag_time` access path. `idx_events_dtag` leads with `pubkey`, so it
-- cannot seek a `(kind, d_tag)` scan that spans authors; this one can.
CREATE INDEX IF NOT EXISTS idx_events_kind_dtag ON events (kind, d_tag, created_at DESC, id ASC);
-- NIP-40 expiry reaper (ascending).
CREATE INDEX IF NOT EXISTS idx_events_expires ON events (expires_at) WHERE expires_at IS NOT NULL;

-- Single-letter tag index rows (one per (event, tag-name, tag-value)). The
-- redundant pubkey / kind / created_at columns let one table serve the tci /
-- atci / ktci access paths via the composite indexes below.
CREATE TABLE IF NOT EXISTS event_tags (
    event_id   BLOB NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    tag_name   TEXT NOT NULL,
    tag_value  TEXT NOT NULL,
    pubkey     BLOB NOT NULL,
    kind       INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tags_tci   ON event_tags (tag_name, tag_value, created_at DESC, event_id ASC);
CREATE INDEX IF NOT EXISTS idx_tags_atci  ON event_tags (pubkey, tag_name, tag_value, created_at DESC, event_id ASC);
CREATE INDEX IF NOT EXISTS idx_tags_ktci  ON event_tags (kind, tag_name, tag_value, created_at DESC, event_id ASC);
-- Drop a removed event's tag rows in O(matching) on delete/replace.
CREATE INDEX IF NOT EXISTS idx_tags_event ON event_tags (event_id);

-- NIP-09 per-id tombstones (mirror of LMDB `deleted-ids`, but storing the full
-- row instead of a presence bit).
CREATE TABLE IF NOT EXISTS tombstones (
    target_id      BLOB PRIMARY KEY,
    kind5_event_id BLOB,
    deleter_pubkey BLOB,
    deleted_at     INTEGER NOT NULL,
    origin         INTEGER NOT NULL,
    source         TEXT
) WITHOUT ROWID;

-- NIP-09 addressable (a-tag) coordinate tombstones (mirror of LMDB
-- `deleted-coordinates`). `coord` is `kind(BE4) || pubkey(32) || d-tag bytes`.
CREATE TABLE IF NOT EXISTS addr_tombstones (
    coord          BLOB PRIMARY KEY,
    kind5_event_id BLOB,
    deleter_pubkey BLOB,
    deleted_at     INTEGER NOT NULL,
    origin         INTEGER NOT NULL,
    source         TEXT
) WITHOUT ROWID;

-- Per-(event, relay) provenance ("events seen on"). The (event_id) and
-- (relay_url) prefixes of the indexes serve provenance_for / list_events_seen_on.
CREATE TABLE IF NOT EXISTS provenance (
    event_id      BLOB NOT NULL,
    relay_url     TEXT NOT NULL,
    first_seen_ms INTEGER NOT NULL,
    last_seen_ms  INTEGER NOT NULL,
    is_primary    INTEGER NOT NULL,
    PRIMARY KEY (event_id, relay_url)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS idx_prov_relay ON provenance (relay_url, event_id);

-- Append-only ingest journal with a monotonic seq. AUTOINCREMENT guarantees the
-- seq never reuses a value even after trim (mirror of LMDB's `last_seq` meta).
CREATE TABLE IF NOT EXISTS ingest_log (
    seq            INTEGER PRIMARY KEY AUTOINCREMENT,
    op             TEXT NOT NULL,
    event_id       BLOB NOT NULL,
    target_id      BLOB,
    reason         TEXT,
    raw_event      BLOB,
    source_relay   TEXT,
    received_at_ms INTEGER NOT NULL
);

-- ─────────────────────────────────────────────────────────────────────────────
-- #1007 PR-5: GC LRU bookkeeping, the K3 coverage ledger, the F-TTL replaceable
-- freshness cache, NMP domain rows + namespace versions, and the volatile
-- ingest-log retention-claim set. Kept in one block so PR-4's query/provenance
-- work and this PR's write/GC work touch disjoint regions of the DDL.
-- ─────────────────────────────────────────────────────────────────────────────

-- LRU access bookkeeping (mirror of LMDB `nmp-lru-access`): event_id → access
-- sequence (monotonic, allocated from the `lru_seq` row in `nmp_meta`). A point
-- read (`get_by_id`) and `hot_set_hint` stamp a fresh seq; GC Phase-2 evicts the
-- lowest-seq un-pinned events first (un-stamped events sort oldest).
CREATE TABLE IF NOT EXISTS lru_access (
    event_id BLOB PRIMARY KEY,
    seq      INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS idx_lru_seq ON lru_access (seq);

-- K3 coverage ledger (ADR-0056 §3): per (filter_hash, relay) the downward-closed
-- `covered_through` watermark. The relay-agnostic store + per-relay ledger is
-- why GC's D3 backstop lowers the right row when it evicts a covered event.
CREATE TABLE IF NOT EXISTS coverage (
    filter_hash     TEXT NOT NULL,
    relay           TEXT NOT NULL,
    covered_through INTEGER NOT NULL,
    PRIMARY KEY (filter_hash, relay)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS idx_coverage_fh ON coverage (filter_hash);

-- F-TTL replaceable-freshness cache: ReplaceableKey blob → check_again_after
-- (unix ms). Persisted (unlike LMDB's in-memory cache) — a missed stamp only
-- means the next claim re-verifies eagerly, never a wrong answer (D6).
CREATE TABLE IF NOT EXISTS replaceable_freshness (
    rkey              BLOB PRIMARY KEY,
    check_again_after INTEGER NOT NULL
) WITHOUT ROWID;

-- NMP domain rows: one shared table keyed (namespace, user_key) — mirror of the
-- LMDB single `nmp-domain-data` sub-db (avoids exhausting a per-namespace cap).
CREATE TABLE IF NOT EXISTS domain_data (
    namespace TEXT NOT NULL,
    user_key  BLOB NOT NULL,
    value     BLOB NOT NULL,
    PRIMARY KEY (namespace, user_key)
) WITHOUT ROWID;
-- Per-namespace schema version (mirror of LMDB `nmp-domain-versions`).
CREATE TABLE IF NOT EXISTS domain_versions (
    namespace TEXT PRIMARY KEY,
    version   INTEGER NOT NULL
) WITHOUT ROWID;

-- Volatile ingest-log retention claims (ADR-0058 §6): the slowest `Protected`
-- cursors that pin the seq-keyed log GC floor. Replaced wholesale each kernel
-- pass; read inside the append-time trim txn. Not durable understanding — a
-- stale row self-evicts once its lag exceeds `max_lag_entries`.
CREATE TABLE IF NOT EXISTS log_retention_claims (
    after_seq       INTEGER NOT NULL,
    max_lag_entries INTEGER NOT NULL
);
"#;

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm_impl::ensure_schema;

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use super::{SCHEMA_SQL, SCHEMA_VERSION};
    use crate::error::SqliteWasmError;
    use crate::shim::SqliteConn;

    /// Create the schema (idempotent) and reconcile the stored schema version.
    ///
    /// On a fresh database this creates every table/index and stamps
    /// `schema_version = SCHEMA_VERSION`. On an existing database it is a no-op
    /// for the DDL (`IF NOT EXISTS`) and then runs forward migrations. A stored
    /// version **newer** than this build is rejected loudly (D6 fail-loud) rather
    /// than silently misinterpreted.
    pub(crate) fn ensure_schema(conn: &SqliteConn) -> Result<(), SqliteWasmError> {
        // Enforce ON DELETE CASCADE for event_tags and run the DDL as one batch.
        conn.exec("PRAGMA foreign_keys = ON;")?;
        conn.exec(SCHEMA_SQL)?;

        match read_schema_version(conn)? {
            None => stamp_schema_version(conn, SCHEMA_VERSION),
            Some(v) if v > SCHEMA_VERSION => Err(SqliteWasmError::Open(format!(
                "database schema version {v} is newer than supported {SCHEMA_VERSION}"
            ))),
            // v == SCHEMA_VERSION (current) — nothing to migrate yet. When the
            // DDL changes, bump SCHEMA_VERSION and add an `v < N` migration arm
            // here, then re-stamp.
            Some(_) => Ok(()),
        }
    }

    fn read_schema_version(conn: &SqliteConn) -> Result<Option<i64>, SqliteWasmError> {
        let stmt = conn.prepare("SELECT value FROM nmp_meta WHERE key = 'schema_version'")?;
        if stmt.step()? {
            Ok(Some(stmt.column_int64(0)?))
        } else {
            Ok(None)
        }
    }

    fn stamp_schema_version(conn: &SqliteConn, version: i64) -> Result<(), SqliteWasmError> {
        let stmt = conn.prepare(
            "INSERT INTO nmp_meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )?;
        stmt.bind_int64(1, version)?;
        // DDL/meta write — a single step drives the statement to completion.
        stmt.step()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddl_mentions_every_access_path() {
        // Cheap native guard: the const DDL names each LMDB-mirrored index so a
        // careless edit that drops one is caught without a wasm run.
        for needle in [
            "idx_events_ci",
            "idx_events_aci",
            "idx_events_akci",
            "idx_events_kci",
            "idx_events_dtag",
            "idx_events_kind_dtag",
            "idx_events_expires",
            "idx_tags_tci",
            "idx_tags_atci",
            "idx_tags_ktci",
            "AUTOINCREMENT",
        ] {
            assert!(SCHEMA_SQL.contains(needle), "DDL missing {needle}");
        }
        assert_eq!(SCHEMA_VERSION, 1);
    }
}
