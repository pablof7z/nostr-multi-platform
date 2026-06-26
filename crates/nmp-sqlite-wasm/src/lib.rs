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
//! ## This is PR-1 (the spine)
//!
//! Only the crate skeleton, the wasm32-gated dependency wiring, and the
//! [`OpfsSqliteStore`] handle type exist here. There is **no SQLite engine and
//! no vendored artifact yet** — those arrive in PR-2 / PR-3. The module list
//! below is the agreed home for each concern so the follow-up PRs slot in
//! without re-litigating layout:
//!
//! * [`shim`] — wasm-bindgen / OPFS JS interop (wasm32 only).
//! * `schema` — SQL DDL + migrations.
//! * `conv` — wire ⇆ row conversions.
//! * `insert` — event write path.
//! * `query` — filter → SQL query path.
//! * `gc` — bounded-store garbage collection.
//! * `domain` — NMP domain rows (watermarks, claims).
//! * `provenance` — D10 source-relay rows, written in the insert txn.
//! * `ingest_log` — append-only ingest journal.
//! * `delete` — NIP-09 / replaceable / addressable tombstone policy.
//! * `interaction_counters` — aggregate counters (reactions, zaps, …).
//! * `store_impl` — [`OpfsSqliteStore`]'s inherent impl (open, txn helpers) and
//!   the single scoped `unsafe impl Send + Sync` from §3. The `EventStore`
//!   trait impl itself lives in `nmp-store`, not here.
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

/// The shim error type, re-exported for PR-3's `nmp-store` wrapper to convert
/// into `nmp_store::StoreError` at the (cycle-free) seam.
#[cfg(target_arch = "wasm32")]
pub use shim::SqliteWasmError;

mod conv;
mod delete;
mod domain;
mod gc;
mod ingest_log;
mod insert;
mod interaction_counters;
mod provenance;
mod query;
mod schema;
mod store_impl;

/// Handle to the OPFS-backed SQLite event store.
///
/// On wasm32 it owns the SQLite connection behind a `RefCell` (interior
/// mutability for the synchronous `&self` `EventStore` trait — ADR-0054 §3),
/// opened via [`OpfsSqliteStore::open`]. The connection handle is `!Send +
/// !Sync`; the store nonetheless carries a single scoped `unsafe impl Send +
/// Sync` (in `store_impl`) justified by single-Worker-actor ownership and made
/// load-bearing by the `target_feature = "atomics"` `compile_error!` guard
/// above. PR-3 wraps this handle with the `EventStore` impl in `nmp-store`.
///
/// The type name resolves on any target (the spine is target-agnostic), but the
/// engine field, the `open` constructor, and the `unsafe impl` are wasm32-only;
/// off wasm32 the struct is a zero-field marker that nothing constructs.
pub struct OpfsSqliteStore {
    /// The opfs-sahpool SQLite connection, owned by exactly one Worker actor.
    #[cfg(target_arch = "wasm32")]
    db: core::cell::RefCell<shim::SqliteConn>,
}
