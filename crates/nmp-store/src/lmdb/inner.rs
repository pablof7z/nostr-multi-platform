//! Internal sub-db / env handles for the LMDB backend (feature-on only).
//!
//! Extracted from `mod.rs` to keep that file under the 500-LOC hard cap.

use std::sync::atomic::AtomicU64;

use heed::types::Bytes;
use heed::{Database, Env};
use nmp_nostr_lmdb::Lmdb;

/// Internal storage handles shared by every method.
///
/// The `Env` is owned by both `Lmdb` (which opened the upstream 11 dbs on
/// it) and by this struct's sub-db handles. The `Lmdb` clone holds its own
/// `Env` clone — heed's `Env` is internally ref-counted so this is cheap.
pub struct Inner {
    pub(crate) env: Env,
    pub(crate) lmdb: Lmdb,
    /// Map size used when the env was opened.  Stored so that runtime
    /// write errors can produce `StoreError::MapFull { map_size_bytes }`
    /// carrying the exact limit (#1521 / D6-no-secrets).
    pub(crate) map_size: usize,
    /// Max concurrent readers used when the env was opened.  Stored so
    /// that runtime read-txn errors can produce
    /// `StoreError::ReaderExhaustion { max_readers }` (#1521 / D6-no-secrets).
    pub(crate) max_readers: u32,
    /// Per-id provenance: event_id (32 bytes) → bincode(Vec<ProvenanceEntry>).
    pub(crate) provenance: Database<Bytes, Bytes>,
    /// Per-id tombstones with full metadata (NMP-side).
    /// Key: target_id (32 bytes). Value: bincode(TombstoneRow).
    pub(crate) tombstones: Database<Bytes, Bytes>,
    /// Address tombstones for param-replaceable kinds.
    /// Key: "kind:pk_hex:dtag" bytes. Value: bincode(TombstoneRow).
    pub(crate) addr_tombstones: Database<Bytes, Bytes>,
    /// Domain schema versions: namespace bytes → u32 BE.
    pub(crate) domain_versions: Database<Bytes, Bytes>,
    /// Domain data: namespace bytes || 0x00 || key bytes → value bytes.
    pub(crate) domain_data: Database<Bytes, Bytes>,
    /// Relay-author scores: `[32 pubkey bytes][1 url-len u8][N url bytes]` →
    /// `[u32 successes BE][u32 failures BE][u64 last_used_unix_s BE][u64 reserved BE]`.
    /// See `relay_scores.rs` for the encode/decode layer and canonicalization.
    pub(crate) relay_author_scores: Database<Bytes, Bytes>,
    /// V-60 LRU access index: event_id (32 bytes) → seq (8 bytes BE).
    ///
    /// Stamped on insert and on every `get_by_id` hit.  Used by `gc_step`
    /// to identify the least-recently-accessed un-pinned events for eviction.
    ///
    /// Design trade-off: stamping on read converts a read-txn into a write-txn
    /// on `get_by_id`.  We accept this cost only for point-reads (not bulk
    /// scans) to bound write-amplification.  The alternative (wall-clock in
    /// a read-txn) would reintroduce a D7 violation; using a persisted
    /// monotonic counter is the only D7-safe option for LMDB.
    pub(crate) lru_access: Database<Bytes, Bytes>,
    /// Monotonic sequence counter for LRU ordering.  Initialised from
    /// `max(lru_access values) + 1` on open so a crash-restart doesn't
    /// reuse seqs that are already in the db.  `Relaxed` ordering is fine —
    /// each store op holds the `heed::RwTxn` lock so there is no concurrent
    /// writer.
    pub(crate) lru_seq: AtomicU64,

    /// V-118 expiration index: expiry_ts_be(8) || event_id(32) → empty.
    ///
    /// Key encoding: expiry timestamp as 8-byte big-endian u64 (NOT inverted,
    /// so lower timestamps sort first — range scan for entries ≤ now_secs is
    /// a simple prefix scan up to `[now_secs+1; 0..0]`).
    ///
    /// Maintained by: insert (if event has expiration tag), all delete paths.
    /// Backfilled on store open for existing databases (V-118).
    ///
    /// This replaces the V-117 `gc_phase1_cursor` heuristic: Phase 1 now does
    /// an O(expired) range scan on this index rather than an O(store) scan of
    /// all events, eliminating the same-`created_at`-block stall (#1097).
    pub(crate) expiry_index: Database<Bytes, Bytes>,

    /// V-52 relay-origin reverse index: `relay_url || 0x00 || event_id(32)`
    /// → empty (presence-only).
    ///
    /// Mirrors the `MemEventStore::relay_index` so `list_events_seen_on` is an
    /// O(events-on-relay) prefix range scan on LMDB instead of an O(store)
    /// provenance scan.  The `0x00` separator is safe: relay URLs are valid
    /// UTF-8 (`wss://…`) and never contain a NUL byte, so the separator can
    /// never appear inside the `relay_url` segment — every key is therefore
    /// unambiguously decodable as `(relay_url, event_id)`.
    ///
    /// Maintained by: the provenance write path (`provenance::upsert` adds the
    /// `(relay, id)` entry; `provenance::delete` removes every `(relay, id)`
    /// entry for the id by first reading the event's provenance relays).
    /// Backfilled once on store open for pre-V-52 databases — see
    /// `open.rs::backfill_relay_index`.
    pub(crate) relay_index: Database<Bytes, Bytes>,

    /// #1518 relay×kind presence index (`nmp-relay-kind`): `relay_url ||
    /// 0x00 || kind(BE4) || event_id(32)` → empty (presence-only).
    ///
    /// A derived projection of per-event provenance committed in the SAME
    /// `RwTxn` as every provenance write — it tells the kernel which kinds
    /// (and how many events of each) a relay has actually served, without
    /// splitting store ownership.  Privacy-gated: NIP-04/17/59 kinds
    /// (4/13/14/15/1059/1060) never enter the index (checked at write time
    /// in `provenance::relay_kind_put`).
    ///
    /// Maintained by the same provenance write path as `relay_index`
    /// (`provenance::upsert` / `provenance::delete`).  Backfilled once on
    /// open for pre-#1518 databases — see `open.rs::backfill_relay_kind_index`.
    pub(crate) relay_kind: Database<Bytes, Bytes>,

    /// K3 coverage ledger (ADR-0056 §3, Stage D1): `filter_hash || 0x1F ||
    /// relay_url` → `covered_through` (8-byte BE unix-seconds).
    ///
    /// Records, per `(filter_hash, relay)`, the downward-closed timestamp
    /// through which a sync has COMPLETED (EOSE on an un-floored REQ, or
    /// NEG-DONE). Written by `EventStore::record_coverage`, read by
    /// `EventStore::get_coverage` — the kernel's since-floor source. This is
    /// the purpose-built successor to the #1090-deleted `nmp-watermarks`
    /// sub-db — re-created with real readers/writers, not re-activated
    /// (ADR-0056 §2.1).
    pub(crate) coverage: Database<Bytes, Bytes>,

    /// Interaction-counter sidecar (issue #1519).
    ///
    /// Key: `target_event_id(32) || counter_kind(1)`.
    /// Value: `count(8 bytes, big-endian u64)`.
    ///
    /// Written atomically with event inserts and removes — same `RwTxn`
    /// (ADR-0011). Counts are always consistent with the stored event set.
    pub(crate) interaction_counters: Database<Bytes, Bytes>,

    /// True when the `nmp-interaction-counters` sub-db schema version is
    /// known (version == 1). Set false if an unknown future version is
    /// detected on open, causing reads to fall back to
    /// `TargetInteractionCounts::default()` (forward-compat safeguard).
    pub(crate) interaction_counters_usable: bool,

    // ── ADR-0058 §4 ingest-log sub-dbs ───────────────────────────────────────
    /// Ingest-log store: seq(8 BE) → JSON(LogEntryPersist).
    pub(crate) ingest_log: Database<Bytes, Bytes>,
    /// Ingest-log metadata: "last_seq" / "gc_floor" → u64 BE.
    pub(crate) ingest_meta: Database<Bytes, Bytes>,

    // ── GC scan state (V-117 fixes) ───────────────────────────────────────
    /// Phase-3/3b tombstone-purge gate: unix_secs of the last pass that
    /// actually ran the tombstone scan.  The Phase-3 scan iterates and
    /// serde-decodes every tombstone row inside a WRITE txn — budget-bound
    /// on count but still O(tombstones) per call.  Throttle to at most once
    /// per `GC_TOMBSTONE_PURGE_INTERVAL_SECS` (1 h) so the 60-second gc tick
    /// does not iterate the full tombstone db every minute.
    ///
    /// Value 0 means "never run" (correct: a 90-day tombstone age threshold
    /// means nothing is purgeable in the first hour anyway).
    pub(crate) gc_last_tombstone_purge_secs: AtomicU64,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner").field("lmdb", &"<Lmdb>").finish()
    }
}
