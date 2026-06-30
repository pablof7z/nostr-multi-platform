//! `EventStore` trait and `DomainHandle` type.
//!
//! Lives in `events.rs` because `trait` is a Rust keyword.
//! See `docs/design/lmdb/trait.md` for the full specification.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::ControlFlow;
use std::sync::Arc;

use nostr::SingleLetterTag;

use super::reference_counts::{ReferenceClassifyFn, TargetReferenceCounts};
use super::text_search::{
    CompiledIndexSpec, SearchScopeId, TextSearchHit, TextSearchQuery, TextSearchStatus,
};
use super::types::{
    CoverageGuard, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport, InsertOutcome,
    ProvenanceEntry, PubKey, RelayUrl, StoreQuery, StoredEvent, TombstoneRow, VerifiedEvent,
};
use super::StoreError;
use crate::domain_handle::DomainHandle;
use crate::DomainMigration;
use crate::ReplaceableKey;

// ─── EventIter ────────────────────────────────────────────────────────────────

/// Lazy iterator over stored events — implementations must be `Send` so the
/// planner can page results across thread boundaries.
pub trait EventIter: Iterator<Item = Result<StoredEvent, StoreError>> + Send {}
impl<T: Iterator<Item = Result<StoredEvent, StoreError>> + Send> EventIter for T {}

// ─── EventStore trait ─────────────────────────────────────────────────────────

/// The single storage abstraction for all Nostr events.
///
/// Backends: `MemEventStore` (always), `LmdbEventStore` (feature = "lmdb-backend").
/// See `docs/design/lmdb/trait.md` for invariant documentation.
pub trait EventStore: Send + Sync {
    // ─── Reads ───────────────────────────────────────────────────────────────

    /// Primary lookup. Returns `Ok(None)` if absent; tombstones do not count as "present".
    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError>;

    /// Pure point-read — returns the event if present and not tombstoned.
    ///
    /// MUST NOT stamp the LRU access counter or open a write transaction.
    /// Use this instead of `get_by_id` when the caller only needs to inspect the event
    /// without influencing GC victim selection (e.g. replay paths that must not bias LRU).
    fn peek_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError>;

    /// `idx_author_kind` scan, newest-first.
    ///
    /// `kinds` must be non-empty; callers wanting any-kind use `scan_by_kind_time` instead.
    fn scan_by_author_kind<'a>(
        &'a self,
        author: &PubKey,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_author_kind` (multi-author) scan, newest-first across the combined author set.
    ///
    /// Results are globally sorted by `(created_at desc, id asc)` across all authors,
    /// duplicate event IDs suppressed. Required (no default) — every backend implements
    /// it natively, exactly like the single-author [`Self::scan_by_author_kind`]; there
    /// is no fan-out fallback so there is only ever one implementation per backend.
    fn scan_by_authors_kind<'a>(
        &'a self,
        authors: &BTreeSet<PubKey>,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_kind_dtag` lookup — returns the current parameterized replaceable for
    /// `(pubkey, kind, d_tag)`, or `Ok(None)`.
    fn get_param_replaceable(
        &self,
        pubkey: &PubKey,
        kind: u32,
        d_tag: &[u8],
    ) -> Result<Option<StoredEvent>, StoreError>;

    /// `idx_kind_dtag_time` scan, newest-first across all authors for `(kind, d_tag)`.
    fn scan_by_kind_dtag<'a>(
        &'a self,
        kind: u32,
        d_tag: &[u8],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// Generic single-letter tag scan, newest-first — the one read path for
    /// every single-letter tag dimension (`#e`, `#p`, `#h`, `#t`, `#a`, …).
    ///
    /// Wildcard semantics (see [`StoreQuery::Tags`] for the full contract):
    /// empty `authors` = any author; empty `kinds` = any kind. `tags` is an
    /// AND-across-keys / OR-within-values map of exact-string tag values.
    /// `since`/`until` are inclusive bounds. An empty `tags` map (or any entry
    /// with an empty value set) yields an empty iterator.
    fn scan_by_tags<'a>(
        &'a self,
        authors: &BTreeSet<PubKey>,
        kinds: &[u32],
        tags: &BTreeMap<SingleLetterTag, BTreeSet<String>>,
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_kind_time` scan, newest-first.
    ///
    /// Pass `kinds = &[]` to scan all kinds (the only scan method that accepts an empty slice).
    fn scan_by_kind_time<'a>(
        &'a self,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// Streaming query: invoke `visitor` once per matching event, newest-first,
    /// up to `limit` events. The visitor returns [`ControlFlow::Break`] to stop
    /// the scan early without materializing the remaining results.
    ///
    /// The visitor receives `&StoredEvent` by reference — no per-event clone or
    /// result-vector allocation occurs on the visit path (D8: working set
    /// bounded, zero per-event alloc after warmup). This default implementation
    /// routes through the matching `scan_by_*` index (so the index logic is not
    /// duplicated); backends may override it to avoid the scan's intermediate
    /// result buffer entirely (see `MemEventStore`).
    ///
    /// Design: `docs/design/nostrdb-notedeck-lessons.md` §2.3 (`ndb_query_visit`).
    fn query_visit(
        &self,
        query: &StoreQuery,
        limit: usize,
        visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
    ) -> Result<(), StoreError> {
        // Dispatch body lives in `events_query_dispatch` to keep this trait file
        // under the file-size hard cap; behaviour is identical.
        crate::events_query_dispatch::query_visit_default(self, query, limit, visitor)
    }

    /// Vec-returning query — a thin wrapper over [`query_visit`](Self::query_visit)
    /// so the index logic lives in exactly one place. Materializes matched
    /// events into a `Vec`, newest-first, capped at `limit`.
    fn query(&self, query: &StoreQuery, limit: usize) -> Result<Vec<StoredEvent>, StoreError> {
        let mut out: Vec<StoredEvent> = Vec::new();
        self.query_visit(query, limit, &mut |ev| {
            out.push(ev.clone());
            ControlFlow::Continue(())
        })?;
        Ok(out)
    }

    /// `idx_expires` scan, ascending — used by the NIP-40 reaper.
    fn scan_expiring_before<'a>(
        &'a self,
        unix_seconds: u64,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// Tombstones referencing a target id (typically one row).
    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError>;

    /// Iterate all tombstones (used by `nmp dump`).
    fn list_tombstones<'a>(
        &'a self,
    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>;

    /// Provenance sidecar for an event.
    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError>;

    /// V-52 — Return the ids of events whose provenance includes `relay_url`.
    ///
    /// Both backends maintain a secondary relay-origin reverse index so this is
    /// an O(events-on-relay) lookup: the `MemEventStore` keeps a
    /// `relay_url → event_ids` map; the `LmdbEventStore` keeps an
    /// `nmp-relay-index` sub-db keyed `relay_url || 0x00 || event_id` and scans
    /// the relay's prefix range (issue #969).
    ///
    /// Both backends return the same set; only the events still present in the
    /// store appear (every removal path prunes the reverse index).
    fn list_events_seen_on(&self, relay_url: &str) -> Result<Vec<EventId>, StoreError>;

    /// #1518 — the kinds a relay has actually served, ascending, deduplicated.
    ///
    /// Derived from the per-event provenance×kind projection both backends
    /// maintain (`MemEventStore` keeps a `relay → kind → ids` map; the
    /// `LmdbEventStore` keeps the `nmp-relay-kind` sub-db and scans the relay's
    /// prefix range).  Privacy-gated kinds (NIP-04/17/59) never appear — they
    /// are excluded at write time.  Default empty so a non-overriding backend
    /// compiles unchanged; both shipped backends override it.
    fn relay_kind_coverage(&self, _relay_url: &str) -> Result<Vec<u32>, StoreError> {
        Ok(Vec::new())
    }

    /// #1518 — how many distinct events of `kind` a relay has served.
    ///
    /// Same projection as [`relay_kind_coverage`](Self::relay_kind_coverage).
    /// A privacy-gated kind always returns `0` (never indexed). Default `0` so a
    /// non-overriding backend compiles unchanged; both shipped backends override
    /// it.
    fn relay_kind_count(&self, _relay_url: &str, _kind: u32) -> Result<u64, StoreError> {
        Ok(0)
    }

    // ─── Writes ──────────────────────────────────────────────────────────────

    /// The single insert path.
    ///
    /// `source` is the relay that delivered this copy. Applies §7.1 invariants,
    /// updates secondaries + provenance + tombstones atomically.
    /// Returns `InsertOutcome` per §7.1.
    ///
    /// Callers must verify the event before calling this method; `VerifiedEvent`
    /// is the proof-of-verification token.
    fn insert(
        &self,
        event: VerifiedEvent,
        source: &RelayUrl,
        received_at_ms: u64,
    ) -> Result<InsertOutcome, StoreError>;

    /// Delete by a NMP-internal filter — for admin / GC / kind:5 application.
    ///
    /// Returns the number of primary rows removed.
    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError>;

    // ─── Hot-set / GC ────────────────────────────────────────────────────────

    /// Soft hint: keep these in hot LRU on a best-effort basis.
    fn hot_set_hint(&self, ids: &[EventId]) -> Result<(), StoreError>;

    /// One bounded GC pass with an explicit derived pin set.
    ///
    /// `pins` is the set of event ids the caller wants to protect from LRU
    /// eviction (Phase 2). The kernel derives this from `timeline`,
    /// `event_claims`, and the active open-interest registry at each GC call
    /// (#1090 Stage 1). The store no longer holds any persisted claim state.
    ///
    /// `now_secs` is the current kernel clock as Unix seconds (D7 — the store
    /// does not read the clock directly; the caller threads it in).
    ///
    /// [`gc_step`](Self::gc_step) is a convenience wrapper that calls this with
    /// an empty pin set — suitable for tests and contexts where no events need
    /// protection.
    fn gc_step_with_pins(
        &self,
        budget: GcBudget,
        now_secs: u64,
        pins: &HashSet<EventId>,
    ) -> Result<GcReport, StoreError>;

    /// One bounded GC pass with no pinned events — reap expired, trim LRU,
    /// purge old tombstones.
    ///
    /// Thin wrapper over [`gc_step_with_pins`](Self::gc_step_with_pins) that
    /// passes an empty pin set. Used by tests and any context that protects no
    /// events. `now_secs` is the current kernel clock as Unix seconds (D7).
    fn gc_step(&self, budget: GcBudget, now_secs: u64) -> Result<GcReport, StoreError> {
        self.gc_step_with_pins(budget, now_secs, &HashSet::new())
    }

    /// K3 Stage D3 (ADR-0056 §3.D3) — one bounded GC pass with both an explicit
    /// pin set AND the eviction⇄ledger coherence backstop.
    ///
    /// Identical to [`gc_step_with_pins`](Self::gc_step_with_pins), but if
    /// Phase-2 LRU eviction deletes an event a `CoverageGuard` matches whose
    /// `created_at <= covered_through`, the matching ledger row's
    /// `covered_through` is lowered to just below the oldest evicted covered
    /// event **in the same transaction as the delete** — so the ledger never
    /// claims coverage of a range it no longer holds (the permanent backfill
    /// hole the memory review flagged). See [`CoverageGuard`].
    ///
    /// `guards` is derived by the kernel on each GC pass from the live coverage
    /// rows + active interest registry. When there are no covered
    /// `(filter_hash, relay)` rows the slice is **empty** and this method is
    /// byte-identical to `gc_step_with_pins`.
    ///
    /// Default impl ignores `guards` and delegates to `gc_step_with_pins`, so a
    /// non-overriding backend compiles unchanged; both shipped backends
    /// (`MemEventStore`, `LmdbEventStore`) override it with the atomic backstop.
    fn gc_step_with_pins_and_coverage(
        &self,
        budget: GcBudget,
        now_secs: u64,
        pins: &HashSet<EventId>,
        _guards: &[CoverageGuard],
    ) -> Result<GcReport, StoreError> {
        self.gc_step_with_pins(budget, now_secs, pins)
    }

    /// K3 Stage D3 — the highest `covered_through` recorded for `filter_hash`
    /// across ALL relays, or `None` if no relay has a coverage row for it.
    ///
    /// The store is relay-agnostic (events are not tagged by which relay covered
    /// them), but the ledger is per-`(filter_hash, relay)`. The floor-coherent
    /// pin set must protect every event a REQ for this shape could floor away on
    /// ANY relay, so it pins below the MAX coverage across the shape's relays
    /// (over-pinning is always safe — it only defers eviction; under-pinning
    /// punches a hole). Default `None` (non-overriding backends / no rows).
    fn coverage_max_for_filter_hash(&self, _filter_hash: &str) -> Option<u64> {
        None
    }

    /// K3 Stage D3 — every `(relay, covered_through)` row recorded for
    /// `filter_hash`, in arbitrary order. Used by the kernel to build one
    /// [`CoverageGuard`] per covered `(filter_hash, relay)` so the eviction
    /// backstop can lower the right row. Default empty (non-overriding backends
    /// / no rows).
    fn coverage_rows_for_filter_hash(&self, _filter_hash: &str) -> Vec<(String, u64)> {
        Vec::new()
    }

    // ─── Domain rows ─────────────────────────────────────────────────────────

    /// Open a module-scoped domain handle.
    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle, StoreError>;

    /// Run schema migrations for a domain namespace.
    fn run_migrations(
        &self,
        namespace: &'static str,
        target_version: u32,
        migrations: &[DomainMigration],
    ) -> Result<(), StoreError>;

    // ─── F-TTL replaceable freshness ───────────────────────────────────────────

    /// Read the `check_again_after` timestamp (unix milliseconds) for a
    /// replaceable identity, or `None` if it has never been freshness-stamped.
    ///
    /// Default no-op (`None`) so non-LMDB backends (e.g. `MemEventStore`) need
    /// no change — the kernel's TTL gate treats `None` as "due now". The LMDB
    /// backend overrides this with the real in-memory-cache-backed lookup.
    ///
    /// `&self` (interior mutability) — the kernel holds the store as
    /// `Arc<dyn EventStore>`, so a `&mut self` method would be uncallable and
    /// break `dyn`-compatibility. The LMDB cache is an `Arc<Mutex<…>>`.
    fn get_check_again_after(&self, _key: &ReplaceableKey) -> Option<u64> {
        None
    }

    /// Stamp the `check_again_after` timestamp (unix milliseconds) for a
    /// replaceable identity.
    ///
    /// Default no-op so non-LMDB backends need no change. The LMDB backend
    /// overrides this to durably persist the timestamp (opening its own write
    /// transaction internally and updating the in-memory cache only after the
    /// transaction commits successfully — see `LmdbEventStore`).
    ///
    /// Errors are intentionally swallowed at this seam: a freshness-stamp
    /// failure must never block ingest or claim (D6 graceful degrade — a
    /// missed stamp simply means the next claim re-verifies eagerly).
    fn set_check_again_after(&self, _key: ReplaceableKey, _ts_ms: u64) {}

    // ─── K3 coverage ledger (ADR-0056 §3, Stage D1) ────────────────────────────

    /// Advance the downward-closed coverage watermark for `(filter_hash, relay)`
    /// to `max(existing, covered_through)`.
    ///
    /// A row means "a sync covering `[0, covered_through]` has COMPLETED for this
    /// shape on this relay" (EOSE on an un-floored REQ, or NEG-DONE). The
    /// advance is monotonic: a later completion can only raise the proven bound.
    /// See [`crate::CoverageRow`] for the honest-coverage rationale (why a
    /// `since`-floored EOSE must NOT call this).
    ///
    /// The kernel records coverage here at a completion (EOSE for an un-floored
    /// REQ, or NEG-DONE), and reads it back as the since-floor source.
    ///
    /// Default no-op so any non-overriding backend compiles unchanged; both
    /// shipped backends (`MemEventStore`, `LmdbEventStore`) override it. Errors
    /// are swallowed at this seam (D6 graceful degrade — a missed coverage write
    /// only means the Stage D2 read falls back to the presence floor, never a
    /// wrong answer).
    fn record_coverage(&self, _filter_hash: &str, _relay: &str, _covered_through: u64) {}

    /// Read the coverage watermark for `(filter_hash, relay)`, or `None` if no
    /// completed-coverage row exists.
    ///
    /// Stage D2 reads this as the since-floor source; Stage D1 exposes it only
    /// so the write path is testable. Default `None` (non-overriding backends
    /// and the un-recorded case both read as "no coverage").
    fn get_coverage(&self, _filter_hash: &str, _relay: &str) -> Option<u64> {
        None
    }

    // ─── Ingest log (ADR-0058 §3, step 1) ────────────────────────────────────────
    /// The highest seq allocated so far (0 if the log is empty).
    fn latest_ingest_seq(&self) -> Result<u64, StoreError>;
    /// The lowest seq still available in the log, or `None` if the log is empty.
    fn oldest_available_seq(&self) -> Result<Option<u64>, StoreError>;
    /// Scan log entries with `seq > after_seq`, ascending, up to `limit`.
    fn scan_log_since_seq(
        &self,
        after_seq: u64,
        limit: usize,
    ) -> Result<crate::ingest_log::ScanLogResult, StoreError>;

    /// Replace the whole `Protected`-cursor log-retention claim set wholesale
    /// (volatile; pins the seq-keyed log GC floor). Rationale: ADR-0058 §6 and
    /// `pull_cursor.rs`. Default no-op; both shipped ingest-log backends override.
    fn replace_log_retention_claims(&self, _claims: &[crate::ingest_log::LogRetentionClaim]) {}

    // ─── Export ──────────────────────────────────────────────────────────────

    /// Dump all store contents in the requested format.
    fn dump(
        &self,
        out: &mut dyn std::io::Write,
        format: DumpFormat,
    ) -> Result<DumpStats, StoreError>;

    // ─── Generic e-tag reference counters (issue #2512) ──────────────────────

    /// Install the protocol-aware reference classifier at composition time.
    ///
    /// Mirrors [`Self::install_search_index_specs`]: `nmp-relations` compiles
    /// its engagement spec (which kinds count, which NIP-10 marker picks the
    /// target) into the opaque [`ReferenceClassifyFn`] and hands it here. The
    /// store runs the closure at every insert / remove to maintain generic
    /// per-target buckets keyed by the opaque [`crate::ReferenceBucketId`] the
    /// closure returns; it never names a protocol concept (D0).
    ///
    /// Default no-op so a backend without the counter sidecar compiles
    /// unchanged. `LmdbEventStore` and `MemEventStore` override it; the OPFS
    /// backend ships no counter index yet, so the default (empty counts)
    /// applies — exactly the FTS contract.
    fn install_reference_counter_classifier(&self, _classifier: Arc<ReferenceClassifyFn>) {}

    /// Reference counts for `target`, bucketed by the opaque ids the installed
    /// classifier produced.
    ///
    /// Default impl returns an empty [`TargetReferenceCounts`] (no classifier
    /// installed / no sidecar). Both `LmdbEventStore` and `MemEventStore`
    /// override this with real counts from their counter stores.
    fn reference_counts(
        &self,
        target: &EventId,
    ) -> Result<TargetReferenceCounts, StoreError> {
        let _ = target;
        Ok(TargetReferenceCounts::default())
    }

    // ─── Full-text search (issue #1811) ──────────────────────────────────────

    /// Install the compiled, protocol-noun-free search index specs.
    ///
    /// Called once at composition time by `nmp-core`, which compiles its
    /// protocol-aware `SearchScopeProvider`s into [`CompiledIndexSpec`]s
    /// (dropping local-only-private scopes and private/encrypted kinds) and
    /// hands the set to the store. The store runs each spec's opaque `extract`
    /// closure + the shared tokenizer at ingest; it never names a protocol
    /// concept (D0).
    ///
    /// Default no-op so a non-FTS backend compiles unchanged. The
    /// `MemEventStore` overrides this to build its in-memory inverted index;
    /// the Phase-2 `LmdbEventStore` overrides it to register its FTS
    /// sub-databases. `&self` (interior mutability) to match the trait's
    /// `Arc<dyn EventStore>` convention.
    fn install_search_index_specs(&self, _specs: Vec<CompiledIndexSpec>) {}

    /// Read accessor: the installed cache-searchable scopes and the kinds each
    /// one indexes (issue #1811).
    ///
    /// Returns one entry per scope that has a live cache index — exactly the set
    /// [`Self::text_search_visit`] can serve. The caller (cache-serve) uses this
    /// to decide, for a search-bearing interest, whether ANY registered scope
    /// covers the interest's kinds: if so it routes the search to the local
    /// index; if not it leaves the search to relays. The vocabulary is
    /// protocol-noun-free — only opaque [`SearchScopeId`]s and kind integers — so
    /// the kernel never learns which NIP a scope belongs to (D0).
    ///
    /// Default empty so a non-FTS backend reports "no cache scopes" and every
    /// search shape stays relay-served. Both shipping backends (mem + LMDB)
    /// override this to expose their installed cache-eligible scopes. `&self`
    /// (interior mutability) to match the trait's `Arc<dyn EventStore>`
    /// convention.
    fn cache_search_scopes(&self) -> Vec<(SearchScopeId, BTreeSet<u32>)> {
        Vec::new()
    }

    /// Streaming text search over one registered scope (issue #1811).
    ///
    /// Invokes `visitor` once per matching document, ordered per
    /// `query.order`, up to `query.limit`. The visitor returns
    /// [`ControlFlow::Break`] to stop early without materializing the remaining
    /// results (mirrors [`Self::query_visit`]).
    ///
    /// Matching is **token + prefix** (the shared tokenizer): a multi-token
    /// query is AND-combined; all but the trailing token match an indexed token
    /// exactly, and the trailing token matches by prefix. The scan is bounded by
    /// `query.budget` and never degrades to a hidden full-corpus scan.
    ///
    /// Returns an explicit [`TextSearchStatus`]: `Complete`, `Partial` (limit or
    /// budget exhausted), `Unsupported` (default / unknown scope),
    /// `IndexBuilding`, or `StoreError`.
    ///
    /// Default returns [`TextSearchStatus::Unsupported`] (and never calls the
    /// visitor) so a non-FTS backend compiles unchanged — exactly the
    /// "default returns Unsupported" trick the relay-coverage methods use.
    fn text_search_visit(
        &self,
        _query: &TextSearchQuery,
        _visitor: &mut dyn FnMut(TextSearchHit) -> ControlFlow<()>,
    ) -> Result<TextSearchStatus, StoreError> {
        Ok(TextSearchStatus::Unsupported)
    }
}
