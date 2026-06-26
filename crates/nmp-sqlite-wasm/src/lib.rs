//! OPFS-SQLite storage engine for wasm32 — issue #1007.
//!
//! This crate is the browser-persistence sibling of `nmp-nostr-lmdb`: a
//! standalone, wasm32-only SQLite-on-OPFS engine that backs nmp-store's
//! synchronous `EventStore` trait so the wasm build gains durable, indexed
//! event storage instead of the in-memory `MemEventStore`. The design is fixed
//! by [`docs/decisions/0054-web-persistence-opfs-sqlite.md`](../../../docs/decisions/0054-web-persistence-opfs-sqlite.md).
//!
//! ## Dependency direction (mirrors `nmp-nostr-lmdb`)
//!
//! This crate does **not** depend on `nmp-store`. `nmp-store` declares the
//! reverse edge — an optional, wasm32-only dependency on this crate gated by
//! its `opfs-sqlite-backend` feature — so depending back on `nmp-store` would
//! be a Cargo cycle. Exactly as `LmdbEventStore` (the `EventStore` impl) lives
//! in `nmp-store/src/lmdb/` wrapping the engine in `nmp-nostr-lmdb`, the
//! `EventStore` impl that bridges this engine lives in `nmp-store` behind the
//! feature (PR-3), wrapping the [`OpfsSqliteStore`] handle defined here.
//!
//! ## Status: engine inherent methods (#1007 PR-3)
//!
//! PR-1 landed the spine, PR-2 vendored the sqlite.org opfs-sahpool artifact +
//! the wasm shim, and PR-3 (this work) adds the real, complete inherent engine
//! methods: the full schema (mirroring the LMDB index access paths), the
//! transactional [`OpfsSqliteStore::insert`], and the point reads
//! ([`OpfsSqliteStore::get_by_id`] / [`OpfsSqliteStore::peek_by_id`] /
//! [`OpfsSqliteStore::get_param_replaceable`]). The `EventStore` **trait** impl
//! is deliberately *not* here yet — it lands in `nmp-store` in a later PR, once
//! every inherent method exists, so no `todo!()`/`unimplemented!()` stub ever
//! ships inside a trait impl (zero-tolerance no-hacks rule). The module homes:
//!
//! * [`shim`] — wasm-bindgen / OPFS JS interop (wasm32 only).
//! * [`error`] — the crate-local [`SqliteWasmError`] (target-agnostic).
//! * [`schema`] — SQL DDL + migrations.
//! * [`conv`] — wire ⇆ row codec (target-agnostic; native-tested).
//! * [`outcome`] — insert-outcome + id types.
//! * `insert` — transactional event write path.
//! * `query` — scan / streaming-query read paths (#1007 PR-4): the materializing
//!   `scan_by_*` methods + the index-served `query_visit` budget loop.
//! * `gc` — bounded-store garbage collection (later PR).
//! * `domain` — NMP domain rows (watermarks, claims) (later PR).
//! * `provenance` — source-relay rows, written in the insert txn.
//! * `ingest_log` — append-only ingest journal (monotonic seq).
//! * `delete` — NIP-09 kind:5 deletion *application* policy.
//! * `tombstones` — per-id / coordinate tombstone row read+write.
//! * `interaction_counters` — aggregate counters (later PR).
//! * `store_impl` — [`OpfsSqliteStore`]'s inherent impl (open, txn + statement
//!   helpers, point reads) and the single scoped `unsafe impl Send + Sync`.
//!
//! ## Send + Sync soundness (ADR-0054 §3)
//!
//! The store handle is owned by exactly one single-threaded Worker actor; that
//! ownership invariant — not "wasm has no threads" — is what makes the
//! `EventStore: Send + Sync` bound honest for a `RefCell`-backed SQLite handle.
//! Enabling wasm threads (`target_feature = "atomics"`) would break that
//! invariant, so the build is hard-failed below rather than silently made
//! unsound.

// On native, the entire crate cfg-gates down to the `OpfsSqliteStore` stub and
// empty module homes; the wasm interop deps are unused there by design.
#![cfg_attr(not(target_arch = "wasm32"), allow(unused))]
#![warn(missing_docs)]

// ADR-0054 §3 soundness guard: the single-Worker-actor ownership that makes the
// upcoming `unsafe impl Send + Sync` sound is destroyed by wasm threads.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
compile_error!(
    "nmp-sqlite-wasm's single-Worker EventStore Send+Sync is unsound under wasm threads"
);

#[cfg(target_arch = "wasm32")]
pub mod shim;

// Target-agnostic public surface: the error type, the event/row codec, the
// insert-outcome + id types, and the DDL. These compile on every target so the
// `nmp-store` `EventStore` wrapper (the cycle-free seam, a later PR) can name
// them and so the pure codec is unit-tested on native.
pub mod conv;
pub mod error;
pub mod outcome;
pub mod schema;

// Target-agnostic PR-5 value/handle/ledger types — named by the (deferred)
// `nmp-store` `EventStore` wrapper and the conformance harness.
pub mod coverage;
pub mod domain;
pub mod ingest_log;
pub mod types;

mod delete;
mod dump;
mod gc;
mod gc_tombstones;
mod ingest_log_store;
mod insert;
mod interaction_counters;
mod meta;
mod provenance;
mod query;
mod store_impl;
mod tombstones;

/// The engine error type — converted into `nmp_store::StoreError` by the
/// `nmp-store` wrapper at the (cycle-free) seam.
pub use error::SqliteWasmError;
/// The engine's event + stored-event representation (mirrors
/// `nmp_store::{RawEvent, StoredEvent}` at the wrapper seam).
pub use conv::{EngineEvent, StoredEngineEvent};
/// Insert outcomes and id types (mirror `nmp_store::{EventId, PubKey,
/// InsertOutcome, RejectReason, TombstoneOrigin}`).
pub use outcome::{EventId, InsertOutcome, PubKey, RejectReason, TombstoneOrigin};
/// The crate-local read query for [`OpfsSqliteStore::query_visit`] (mirror of
/// `nmp_store::StoreQuery`; the `nmp-store` wrapper maps the two at the seam).
pub use query::EngineQuery;

// ── #1007 PR-5 public surface (gc / coverage ledger / ingest log / dump) ──────
/// The eviction⇄ledger coherence backstop input (mirror `nmp_store::CoverageGuard`).
pub use coverage::{CoverageGuard, CoverageMatchFn};
/// The module-scoped domain handle (`domain_open`'s return).
pub use domain::OpfsDomainHandle;
/// Ingest-journal types (mirror `nmp_store::ingest_log::*`).
pub use ingest_log::{
    DeleteReason, LogOp, LogRetentionClaim, PullGap, PullPage, ScanLogResult, StoreLogEntry,
    DEFAULT_LOG_MAX_ENTRIES,
};
/// GC / delete / dump / freshness / interaction / migration value types
/// (mirror the corresponding `nmp_store` types at the cycle-free seam).
pub use types::{
    DeleteFilter, DomainMigration, DumpStats, GcBudget, GcReport, MigrationTx, ReplaceableKey,
    TargetInteractionCounts,
};

/// Handle to the OPFS-backed SQLite event store.
///
/// On wasm32 it owns the SQLite connection behind a `RefCell` (interior
/// mutability for the synchronous `&self` `EventStore` trait — ADR-0054 §3),
/// opened via [`OpfsSqliteStore::open`]. The connection handle is `!Send +
/// !Sync`; the store nonetheless carries a single scoped `unsafe impl Send +
/// Sync` (in `store_impl`) justified by single-Worker-actor ownership and made
/// load-bearing by the `target_feature = "atomics"` `compile_error!` guard
/// above. A later PR wraps this handle with the `EventStore` impl in `nmp-store`.
///
/// The type name resolves on any target (the spine is target-agnostic), but the
/// engine field, the `open` constructor, and the `unsafe impl` are wasm32-only;
/// off wasm32 the struct is a zero-field marker that nothing constructs.
pub struct OpfsSqliteStore {
    /// The opfs-sahpool SQLite connection, owned by exactly one Worker actor.
    #[cfg(target_arch = "wasm32")]
    db: core::cell::RefCell<shim::SqliteConn>,
}
