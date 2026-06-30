//! Internal sub-db / env handles for the LMDB backend (feature-on only).
//!
//! Extracted from `mod.rs` to keep that file under the 500-LOC hard cap.

use std::sync::atomic::AtomicU64;
use std::sync::RwLock;

use heed::types::Bytes;
use heed::{Database, Env};
use nmp_nostr_lmdb::Lmdb;

use crate::ingest_log::LogRetentionClaim;

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
    /// detected on open, causing reads to fall back to an empty
    /// `TargetReferenceCounts` (forward-compat safeguard).
    pub(crate) interaction_counters_usable: bool,

    /// Installed reference classifier (#2512), set once at composition by
    /// `install_reference_counter_classifier`. `nmp-relations` compiles its
    /// protocol-aware engagement spec into the opaque closure; the store runs it
    /// at every insert/remove inside the same `RwTxn` as the event write. `None`
    /// until installed → the counter sidecar is inert. `RwLock` mirrors the FTS
    /// `fts_specs` seam: single-writer (composition), many-reader (every write).
    /// A poisoned lock degrades to "no counters", never a panic (D6).
    pub(crate) reference_classifier:
        RwLock<Option<std::sync::Arc<crate::reference_counts::ReferenceClassifyFn>>>,

    // ── ADR-0058 §4 ingest-log sub-dbs ───────────────────────────────────────
    /// Ingest-log store: seq(8 BE) → JSON(LogEntryPersist).
    pub(crate) ingest_log: Database<Bytes, Bytes>,
    /// Ingest-log metadata: "last_seq" / "gc_floor" → u64 BE.
    pub(crate) ingest_meta: Database<Bytes, Bytes>,

    // ── #1811 full-text-search inverted index (durable backend of the FTS seam) ─
    /// Inverted postings (`nmp-fts-postings`):
    /// `scope_discriminant(4 BE) || token_bytes || 0x00 || (!created_at)(8 BE) ||
    /// doc_key(32)` → empty.
    ///
    /// The `!created_at` (`u64::MAX - created_at`) segment makes a forward cursor
    /// scan over a token's range yield documents newest-first; the `0x00`
    /// delimiter after the (NUL-free, see `fts.rs`) token bytes makes every key
    /// unambiguously decodable. Maintained in the SAME `RwTxn` as the event write
    /// at every insert/replace/delete/GC site (mirrors `relay_index`).
    pub(crate) fts_postings: Database<Bytes, Bytes>,

    /// Doc → terms (`nmp-fts-doc-terms`): `doc_key(32)` → packed `Vec<token>`.
    ///
    /// Drives DOC-KEY-driven removal: every delete path reads the doc's term list
    /// here and deletes exactly that document's postings WITHOUT re-tokenizing the
    /// (possibly unavailable) event body. Value layout in `fts.rs::encode_terms`.
    pub(crate) fts_doc_terms: Database<Bytes, Bytes>,

    /// Term stats (`nmp-fts-term-stats`): `scope_discriminant(4 BE) || token_bytes`
    /// → doc-frequency(8 BE).
    ///
    /// Lets the query planner pick the RAREST root term to seed the scan (smallest
    /// posting list first), so AND-intersection touches the fewest candidate docs.
    pub(crate) fts_term_stats: Database<Bytes, Bytes>,

    /// Installed FTS specs (#1811), set once at composition by
    /// `install_search_index_specs`. Wrapped in an `RwLock` because the spec set
    /// (with its type-erased extractor closures) is not known until after the
    /// store is opened. Single-writer (composition), many-reader (every event
    /// write + every query). A poisoned lock degrades to "no FTS" (search then
    /// returns `Unsupported`), never a panic (D6).
    pub(crate) fts_specs: RwLock<Vec<crate::text_search::CompiledIndexSpec>>,

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

    /// VOLATILE `Protected`-cursor log-retention claims (ADR-0058 §6, step-4).
    ///
    /// Held in memory only — cursor registrations are non-durable, so their
    /// retention claims are non-durable too (never written to `nmp-ingest-meta`).
    /// Written wholesale by `EventStore::replace_log_retention_claims` (kernel =
    /// single writer). Each event mutation snapshots this (clones the `Vec`) and
    /// passes it into the append-time `trim_in_txn` so the trim sees a consistent
    /// set within the event `RwTxn`.
    pub(crate) retention_claims: RwLock<Vec<LogRetentionClaim>>,
}

impl Inner {
    /// Snapshot the current retention-claim set (ADR-0058 §6, step-4).
    ///
    /// Cloned once per event mutation and threaded into `trim_in_txn` so the
    /// append-time trim sees a consistent claim set within the event `RwTxn`
    /// (the serializing invariant is the write txn, not actor single-threading).
    /// A poisoned lock is non-fatal — fall back to an empty set (normal trim,
    /// the consumer degrades to an explicit `PullGap`, never a silent skip).
    /// Snapshot the installed FTS spec set (#1811) for one event-write txn.
    ///
    /// Cloned once per event mutation so the txn sees a consistent spec set while
    /// indexing. A poisoned lock degrades to an empty set — that mutation simply
    /// writes no FTS rows (search returns Unsupported/empty), never a panic (D6).
    pub(crate) fn fts_specs_snapshot(&self) -> Vec<crate::text_search::CompiledIndexSpec> {
        match self.fts_specs.read() {
            Ok(g) => g.clone(),
            Err(e) => {
                tracing::warn!(error = %e, "fts_specs lock poisoned; indexing with empty spec set");
                Vec::new()
            }
        }
    }

    /// The installed cache-searchable scopes and the kinds each indexes — the
    /// read side of [`crate::EventStore::cache_search_scopes`] (#1811). Mirrors
    /// the mem backend: every spec in `fts_specs` is already cache-eligible
    /// (`SearchScopeRegistry::compile` drops `RelayOnly` / `LocalOnlyPrivate`
    /// scopes and private kinds before install), so the cache-serve hook can
    /// match a search shape against these `(scope, kinds)` pairs without naming
    /// any FTS noun. A poisoned lock degrades to "no cache scopes" (D6 — search
    /// then stays relay-served, never a panic).
    pub(crate) fn fts_cache_scopes(
        &self,
    ) -> Vec<(crate::text_search::SearchScopeId, std::collections::BTreeSet<u32>)> {
        match self.fts_specs.read() {
            Ok(g) => g
                .iter()
                .map(|spec| (spec.scope_id, spec.kinds.clone()))
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "fts_specs lock poisoned; reporting no cache search scopes");
                Vec::new()
            }
        }
    }

    pub(crate) fn retention_claims_snapshot(&self) -> Vec<LogRetentionClaim> {
        match self.retention_claims.read() {
            Ok(g) => g.clone(),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "retention_claims lock poisoned; trimming with empty claim set"
                );
                Vec::new()
            }
        }
    }
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner").field("lmdb", &"<Lmdb>").finish()
    }
}
