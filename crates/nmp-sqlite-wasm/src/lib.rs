//! OPFS-SQLite EventStore backend for wasm32.
//!
//! This crate implements the `EventStore` trait over the Origin Private File System (OPFS)
//! using a vendored sqlite.org WASM module and the SyncAccessHandle pool VFS. wasm32-only.
//!
//! See ADR-0054 for the full architecture specification.

// ─── Atomics guard (ADR-0054 §3) ───────────────────────────────────────────────────
//
// OpfsSqliteStore's `unsafe impl Send+Sync` assumes a single-Worker-actor owns the
// SQLite handle. If wasm threads (atomics) were enabled, `Arc<dyn EventStore>` would
// become shareable across worker threads, making the impl unsound. This compile_error!
// fires if both target_arch="wasm32" and target_feature="atomics" are set.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
compile_error!(
    "OpfsSqliteStore's unsafe impl Send+Sync assumes a single-threaded Worker actor owns the \
     SQLite handle; wasm threads (atomics) would make Arc<dyn EventStore> shareable across \
     worker threads and the impl unsound."
);

// ─── Module declarations ──────────────────────────────────────────────────────────────

// Public shim interface to the JS sqlite3 WASM module
pub mod shim;

// Synchronous wrapper around the SQLite connection
mod conn;

// Schema management and migration
mod schema;

// Type conversions between NMP and SQLite
mod conv;

// Event insertion paths
mod insert;
mod insert_kind5;

// Event query and retrieval
mod query;

// Event deletion
mod delete;

// Garbage collection
mod gc;

// Domain-scoped transactional handles
mod domain;

// Event provenance tracking
mod provenance;

// Ingest log implementation
mod ingest_log;

// Interaction counter tracking
mod interaction_counters;

// Dump/export functionality
mod dump;

// The main `impl EventStore` implementation
mod store_impl;

// ─── Error types ──────────────────────────────────────────────────────────────────────

use std::fmt;

/// Typed open failure, mapped by the composition root to `store_open_failure`.
#[derive(Debug, Clone)]
pub enum OpfsOpenError {
    /// Safari <17.4, private browsing, or OPFS not available.
    SahPoolUnavailable(String),
    /// Second tab cannot acquire exclusive SAH pool lock (multi-tab scenario).
    PoolLockHeld(String),
    /// Storage quota denied by the user or browser policy.
    QuotaDenied(String),
    /// sqlite3.wasm/JS failed to instantiate.
    WasmModuleLoad(String),
    /// Schema initialization or migration failed.
    SchemaInit(String),
    /// Other/unclassified errors.
    Other(String),
}

impl fmt::Display for OpfsOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SahPoolUnavailable(msg) => write!(f, "OPFS SAH pool unavailable: {}", msg),
            Self::PoolLockHeld(msg) => write!(f, "OPFS pool lock held (multi-tab): {}", msg),
            Self::QuotaDenied(msg) => write!(f, "Storage quota denied: {}", msg),
            Self::WasmModuleLoad(msg) => write!(f, "sqlite3.wasm load failed: {}", msg),
            Self::SchemaInit(msg) => write!(f, "Schema init failed: {}", msg),
            Self::Other(msg) => write!(f, "Open error: {}", msg),
        }
    }
}

impl std::error::Error for OpfsOpenError {}

impl OpfsOpenError {
    /// Stable reason string for logging and diagnostics.
    pub fn reason(&self) -> String {
        self.to_string()
    }
}

// ─── The OPFS-SQLite EventStore ───────────────────────────────────────────────────────

/// The OPFS-SQLite EventStore backend.
///
/// Wraps a synchronous SQLite connection opened via OPFS SyncAccessHandle pool VFS.
/// The struct uses `RefCell<SqliteConn>` for interior mutability (not `Mutex`), as the
/// single-Worker-actor invariant (ADR-0047 §1, D4) guarantees only one thread accesses
/// the store at a time.
///
/// This type is wasm32-only and must be instantiated via `OpfsSqliteStore::open()`.
pub struct OpfsSqliteStore {
    // Private: db: RefCell<SqliteConn> — to be filled in by Slice A
    _private: (),
}

impl OpfsSqliteStore {
    /// Schema/index version this build writes (provenance + migration gate).
    pub const SCHEMA_VERSION: u32 = 0;

    /// Asynchronously opens the OPFS-backed SQLite database.
    ///
    /// Called exactly once in the Worker before Start (ADR-0054 §5). Instantiates
    /// the vendored sqlite3 WASM module, registers + async-opens the OPFS SAH pool VFS
    /// keyed by `database_name`, runs schema migrations, and returns a fully-synchronous
    /// handle. wasm32 only.
    ///
    /// # Errors
    ///
    /// Returns `OpfsOpenError` on OPFS unavailability, JS instantiation failure,
    /// lock contention, or schema initialization errors.
    #[cfg(target_arch = "wasm32")]
    pub async fn open(database_name: &str) -> Result<Self, OpfsOpenError> {
        let _ = database_name;
        todo!("Slice A: OPFS SAH pool initialization, SQLite module loading, schema migrations")
    }

    /// Stub for non-wasm32 targets (tests, native builds).
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn open(database_name: &str) -> Result<Self, OpfsOpenError> {
        let _ = database_name;
        Err(OpfsOpenError::Other(
            "OpfsSqliteStore is wasm32-only; cannot open on this target".to_string(),
        ))
    }
}

// SAFETY: single-Worker-actor ownership (ADR-0047 §1, D4).
//
// The ONLY unsafe impl in this crate. Justified by the single-threaded Worker actor
// invariant: only one thread ever accesses the SQLite handle, even though Arc<dyn EventStore>
// is Sync-shareable downstream in nmp-core. The wasm32+atomics guard above prevents
// double-threading.
#[cfg(target_arch = "wasm32")]
unsafe impl Send for OpfsSqliteStore {}

#[cfg(target_arch = "wasm32")]
unsafe impl Sync for OpfsSqliteStore {}

// For non-wasm32 targets, the struct still compiles but Send+Sync are not implemented.
// Tests must use #[cfg(target_arch = "wasm32")] to avoid trait-object issues.

// Implement EventStore for OpfsSqliteStore (Slice B+C fill the bodies).
// The trait impl is cfg-gated to wasm32 only. Slice B+C will implement all the methods.
#[cfg(target_arch = "wasm32")]
impl nmp_store::EventStore for OpfsSqliteStore {
    // ─── Reads ─────────────────────────────────────────────────────────────
    fn get_by_id(&self, id: &nmp_store::EventId) -> Result<Option<nmp_store::StoredEvent>, nmp_store::StoreError> {
        let _ = id;
        todo!("Slice B: query by event id")
    }

    fn peek_by_id(&self, id: &nmp_store::EventId) -> Result<Option<nmp_store::StoredEvent>, nmp_store::StoreError> {
        let _ = id;
        todo!("Slice B: non-stamping peek by id")
    }

    fn scan_by_author_kind<'a>(
        &'a self,
        author: &nmp_store::PubKey,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn nmp_store::EventIter + 'a>, nmp_store::StoreError> {
        let _ = (author, kinds, since, until, limit);
        todo!("Slice B: scan by author and kinds")
    }

    fn scan_by_authors_kind<'a>(
        &'a self,
        authors: &std::collections::BTreeSet<nmp_store::PubKey>,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn nmp_store::EventIter + 'a>, nmp_store::StoreError> {
        let _ = (authors, kinds, since, until, limit);
        todo!("Slice B: scan by multiple authors and kinds")
    }

    fn get_param_replaceable(
        &self,
        pubkey: &nmp_store::PubKey,
        kind: u32,
        d_tag: &[u8],
    ) -> Result<Option<nmp_store::StoredEvent>, nmp_store::StoreError> {
        let _ = (pubkey, kind, d_tag);
        todo!("Slice B: get parameterized replaceable event")
    }

    fn scan_by_kind_dtag<'a>(
        &'a self,
        kind: u32,
        d_tag: &[u8],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn nmp_store::EventIter + 'a>, nmp_store::StoreError> {
        let _ = (kind, d_tag, since, until, limit);
        todo!("Slice B: scan by kind and d-tag")
    }

    fn scan_by_etag<'a>(
        &'a self,
        target: &nmp_store::EventId,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn nmp_store::EventIter + 'a>, nmp_store::StoreError> {
        let _ = (target, kinds, limit);
        todo!("Slice B: scan by e-tag")
    }

    fn scan_by_ptag<'a>(
        &'a self,
        target: &nmp_store::PubKey,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn nmp_store::EventIter + 'a>, nmp_store::StoreError> {
        let _ = (target, kinds, limit);
        todo!("Slice B: scan by p-tag")
    }

    fn scan_by_kind_time<'a>(
        &'a self,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn nmp_store::EventIter + 'a>, nmp_store::StoreError> {
        let _ = (kinds, since, until, limit);
        todo!("Slice B: scan by kind and time")
    }

    fn scan_expiring_before<'a>(
        &'a self,
        unix_seconds: u64,
        limit: usize,
    ) -> Result<Box<dyn nmp_store::EventIter + 'a>, nmp_store::StoreError> {
        let _ = (unix_seconds, limit);
        todo!("Slice B: scan expiring events")
    }

    fn tombstones_for(&self, target: &nmp_store::EventId) -> Result<Vec<nmp_store::TombstoneRow>, nmp_store::StoreError> {
        let _ = target;
        todo!("Slice B: query tombstones for target")
    }

    fn list_tombstones<'a>(
        &'a self,
    ) -> Result<Box<dyn Iterator<Item = Result<nmp_store::TombstoneRow, nmp_store::StoreError>> + Send + 'a>, nmp_store::StoreError> {
        todo!("Slice B: list all tombstones")
    }

    fn provenance_for(&self, id: &nmp_store::EventId) -> Result<Vec<nmp_store::ProvenanceEntry>, nmp_store::StoreError> {
        let _ = id;
        todo!("Slice B: query provenance for event")
    }

    fn list_events_seen_on(&self, relay_url: &str) -> Result<Vec<nmp_store::EventId>, nmp_store::StoreError> {
        let _ = relay_url;
        todo!("Slice B: list events seen on relay")
    }

    fn relay_kind_coverage(&self, relay_url: &str) -> Result<Vec<u32>, nmp_store::StoreError> {
        let _ = relay_url;
        todo!("Slice B: relay kind coverage")
    }

    fn relay_kind_count(&self, relay_url: &str, kind: u32) -> Result<u64, nmp_store::StoreError> {
        let _ = (relay_url, kind);
        todo!("Slice B: relay kind count")
    }

    // ─── Writes ────────────────────────────────────────────────────────────
    fn insert(
        &self,
        event: nmp_store::VerifiedEvent,
        source: &nmp_store::RelayUrl,
        received_at_ms: u64,
    ) -> Result<nmp_store::InsertOutcome, nmp_store::StoreError> {
        let _ = (event, source, received_at_ms);
        todo!("Slice B: insert verified event")
    }

    fn delete_by_filter(&self, filter: nmp_store::DeleteFilter) -> Result<usize, nmp_store::StoreError> {
        let _ = filter;
        todo!("Slice B: delete by filter")
    }

    // ─── Hot-set / GC ──────────────────────────────────────────────────────
    fn hot_set_hint(&self, ids: &[nmp_store::EventId]) -> Result<(), nmp_store::StoreError> {
        let _ = ids;
        todo!("Slice B: hot set hint")
    }

    fn gc_step_with_pins(
        &self,
        budget: nmp_store::GcBudget,
        now_secs: u64,
        pins: &std::collections::HashSet<nmp_store::EventId>,
    ) -> Result<nmp_store::GcReport, nmp_store::StoreError> {
        let _ = (budget, now_secs, pins);
        todo!("Slice B: gc step with pins")
    }

    fn gc_step_with_pins_and_coverage(
        &self,
        budget: nmp_store::GcBudget,
        now_secs: u64,
        pins: &std::collections::HashSet<nmp_store::EventId>,
        guards: &[nmp_store::CoverageGuard],
    ) -> Result<nmp_store::GcReport, nmp_store::StoreError> {
        let _ = (budget, now_secs, pins, guards);
        todo!("Slice B: gc step with pins and coverage")
    }

    fn coverage_max_for_filter_hash(&self, filter_hash: &str) -> Option<u64> {
        let _ = filter_hash;
        todo!("Slice B: coverage max for filter hash")
    }

    fn coverage_rows_for_filter_hash(&self, filter_hash: &str) -> Vec<(String, u64)> {
        let _ = filter_hash;
        todo!("Slice B: coverage rows for filter hash")
    }

    // ─── Domain rows ───────────────────────────────────────────────────────
    fn domain_open(&self, namespace: &'static str) -> Result<nmp_store::DomainHandle, nmp_store::StoreError> {
        let _ = namespace;
        todo!("Slice B: domain open")
    }

    fn run_migrations(
        &self,
        namespace: &'static str,
        target_version: u32,
        migrations: &[nmp_store::DomainMigration],
    ) -> Result<(), nmp_store::StoreError> {
        let _ = (namespace, target_version, migrations);
        todo!("Slice B: run migrations")
    }

    // ─── F-TTL replaceable freshness ───────────────────────────────────────
    fn get_check_again_after(&self, key: &nmp_store::ReplaceableKey) -> Option<u64> {
        let _ = key;
        None
    }

    fn set_check_again_after(&self, key: nmp_store::ReplaceableKey, ts_ms: u64) {
        let _ = (key, ts_ms);
        // Stub: no-op
    }

    // ─── K3 coverage ledger ────────────────────────────────────────────────
    fn record_coverage(&self, filter_hash: &str, relay: &str, covered_through: u64) {
        let _ = (filter_hash, relay, covered_through);
        todo!("Slice B: record coverage")
    }

    fn get_coverage(&self, filter_hash: &str, relay: &str) -> Option<u64> {
        let _ = (filter_hash, relay);
        todo!("Slice B: get coverage")
    }

    // ─── Ingest log ────────────────────────────────────────────────────────
    fn latest_ingest_seq(&self) -> Result<u64, nmp_store::StoreError> {
        todo!("Slice B: latest ingest seq")
    }

    fn oldest_available_seq(&self) -> Result<Option<u64>, nmp_store::StoreError> {
        todo!("Slice B: oldest available seq")
    }

    fn scan_log_since_seq(
        &self,
        after_seq: u64,
        limit: usize,
    ) -> Result<nmp_store::ScanLogResult, nmp_store::StoreError> {
        let _ = (after_seq, limit);
        todo!("Slice B: scan log since seq")
    }

    fn replace_log_retention_claims(&self, claims: &[nmp_store::LogRetentionClaim]) {
        let _ = claims;
        todo!("Slice B: replace log retention claims")
    }

    // ─── Export ────────────────────────────────────────────────────────────
    fn dump(
        &self,
        out: &mut dyn std::io::Write,
        format: nmp_store::DumpFormat,
    ) -> Result<nmp_store::DumpStats, nmp_store::StoreError> {
        let _ = (out, format);
        todo!("Slice B: dump store")
    }

    fn interaction_counts(
        &self,
        target: &nmp_store::EventId,
    ) -> Result<nmp_store::TargetInteractionCounts, nmp_store::StoreError> {
        let _ = target;
        todo!("Slice B: interaction counts")
    }

    // ─── Full-text search ──────────────────────────────────────────────────
    fn install_search_index_specs(&self, specs: Vec<nmp_store::CompiledIndexSpec>) {
        let _ = specs;
        todo!("Slice B: install search index specs")
    }

    fn cache_search_scopes(&self) -> Vec<(nmp_store::SearchScopeId, std::collections::BTreeSet<u32>)> {
        todo!("Slice B: cache search scopes")
    }

    fn text_search_visit(
        &self,
        query: &nmp_store::TextSearchQuery,
        visitor: &mut dyn FnMut(nmp_store::TextSearchHit) -> std::ops::ControlFlow<()>,
    ) -> Result<nmp_store::TextSearchStatus, nmp_store::StoreError> {
        let _ = (query, visitor);
        todo!("Slice B: text search visit")
    }
}
