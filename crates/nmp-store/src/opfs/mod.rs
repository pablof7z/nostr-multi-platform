//! `EventStore` backend over the OPFS-SQLite engine (`nmp_sqlite_wasm`) (#1007).
//!
//! Sibling of [`crate::lmdb`]: the trait owner is `nmp-store`, and the engine
//! crate cannot depend back on it (Cargo cycle), so the `impl EventStore` lives
//! here, wrapping `nmp_sqlite_wasm::OpfsSqliteStore` and adapting every
//! crate-local engine type to the `nmp-store` vocabulary at the [`conv`] seam.
//!
//! The whole module is gated `cfg(all(target_arch = "wasm32", feature =
//! "opfs-sqlite-backend"))`: the engine dependency is wasm32-only (its inherent
//! methods are `#[cfg(target_arch = "wasm32")]`), and on native the feature
//! resolves to nothing — so a native `--all-features` build excludes this
//! module entirely and keeps using the LMDB / in-memory backends.

mod conv;
mod store_impl;

pub use store_impl::OpfsSqliteEventStore;

/// Engine `SqliteWasmError` → `StoreError`, re-exported for the
/// [`crate::domain_handle::DomainHandle`] OPFS arm (the one crate-internal
/// consumer outside this module).
pub(crate) use conv::store_err;
