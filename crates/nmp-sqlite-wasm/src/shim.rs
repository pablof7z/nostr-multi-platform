//! wasm-bindgen / OPFS JavaScript interop shim (wasm32 only).
//!
//! This is the safe Rust face of the hand-written shim. It sits on top of the
//! raw extern bindings in [`sqlite3_bindings`] (which bind the vendored JS glue
//! `vendor/sqlite-wasm/nmp-sqlite3-shim.mjs`) and exposes a small, `Result`-typed
//! API — open / exec / the prepared-statement cycle — that later PRs (the
//! `nmp-store` `EventStore` impl) call. No schema, no insert/query logic lives
//! here; this PR delivers only the engine bridge (#1007 PR-2).
//!
//! ## Engine: opfs-sahpool, async-at-open then synchronous (ADR-0054 §1)
//!
//! The backend is the official sqlite.org WASM build over the OPFS
//! SyncAccessHandle *pool* VFS. The pool is opened **async exactly once**
//! ([`SqliteConn::open`]); every subsequent statement is synchronous, which is
//! what lets a `RefCell`-backed handle satisfy the synchronous `&self`
//! `EventStore` trait. opfs-sahpool needs **no** COOP/COEP cross-origin
//! isolation and **no** SharedArrayBuffer, unlike the older async `opfs` VFS.
//!
//! ## Errors
//!
//! Every fallible call maps a thrown JS exception (`JsValue`) onto
//! [`SqliteWasmError`], a crate-local enum. It is deliberately **not**
//! `nmp_store::StoreError`: this crate cannot depend on `nmp-store` (that would
//! be a Cargo cycle — see the crate-level docs). PR-3's `nmp-store` wrapper owns
//! the `SqliteWasmError -> StoreError` conversion at the seam where the
//! dependency direction makes it legal.

mod sqlite3_bindings;

use core::marker::PhantomData;
use sqlite3_bindings as raw;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// Error returned by the SQLite-on-OPFS shim.
///
/// Crate-local on purpose (no `nmp-store` dependency — a Cargo cycle would
/// result). PR-3's `nmp-store` `EventStore` wrapper converts these into
/// `nmp_store::StoreError` at the boundary: `ModuleInit` / `VfsInstall` / `Open`
/// / `Close` are backend-i/o failures (`StoreError::Io`), `Exec` / `Prepare` /
/// `Bind` / `Step` are statement faults (`StoreError::Io` or `Corrupt`), and
/// `Column` is a decode fault (`StoreError::Encoding`). The wrapped `String` is
/// the stringified JS exception; per D6 it carries no private event content.
#[derive(Debug, Clone)]
pub enum SqliteWasmError {
    /// `sqlite3InitModule()` failed to instantiate the WASM module.
    ModuleInit(String),
    /// Installing/registering the opfs-sahpool VFS failed.
    VfsInstall(String),
    /// Opening the database file on the pool VFS failed.
    Open(String),
    /// Closing the database handle failed.
    Close(String),
    /// A no-result `exec` (DDL / pragma) failed.
    Exec(String),
    /// Compiling a statement (`prepare`) failed.
    Prepare(String),
    /// Binding a parameter value failed.
    Bind(String),
    /// Stepping the statement failed.
    Step(String),
    /// Reading/decoding a result column failed.
    Column(String),
}

impl core::fmt::Display for SqliteWasmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ModuleInit(s) => write!(f, "sqlite wasm module init failed: {s}"),
            Self::VfsInstall(s) => write!(f, "opfs-sahpool vfs install failed: {s}"),
            Self::Open(s) => write!(f, "open database failed: {s}"),
            Self::Close(s) => write!(f, "close database failed: {s}"),
            Self::Exec(s) => write!(f, "exec failed: {s}"),
            Self::Prepare(s) => write!(f, "prepare failed: {s}"),
            Self::Bind(s) => write!(f, "bind failed: {s}"),
            Self::Step(s) => write!(f, "step failed: {s}"),
            Self::Column(s) => write!(f, "column read failed: {s}"),
        }
    }
}

impl core::error::Error for SqliteWasmError {}

/// Stringify a thrown `JsValue` for an error message. D6: no private event
/// content reaches this path — only engine-level exception text.
fn js_err(value: JsValue) -> String {
    if let Some(s) = value.as_string() {
        return s;
    }
    if let Some(err) = value.dyn_ref::<js_sys::Error>() {
        return String::from(&err.message());
    }
    format!("{value:?}")
}

/// A connection to a SQLite database opened on the opfs-sahpool VFS.
///
/// `!Send + !Sync` (it holds a `JsValue` engine handle). Soundness of the
/// single scoped `unsafe impl Send + Sync` on [`crate::OpfsSqliteStore`] rests
/// on this handle being owned and observed by exactly one Web Worker actor
/// (ADR-0054 §3); the `target_feature = "atomics"` `compile_error!` guard in the
/// crate root makes that invariant load-bearing.
pub struct SqliteConn {
    db: JsValue,
}

impl SqliteConn {
    /// Initialise the engine, install the opfs-sahpool VFS, and open (creating
    /// if absent) the database named `database_name` on that VFS.
    ///
    /// Async: this is the one-time async pool-open boundary. Module init and VFS
    /// install are idempotent across calls, so opening a second database is
    /// cheap.
    pub async fn open(database_name: &str) -> Result<Self, SqliteWasmError> {
        JsFuture::from(raw::init())
            .await
            .map_err(|e| SqliteWasmError::ModuleInit(js_err(e)))?;
        JsFuture::from(raw::install_pool_vfs())
            .await
            .map_err(|e| SqliteWasmError::VfsInstall(js_err(e)))?;
        let db = raw::open_db(database_name).map_err(|e| SqliteWasmError::Open(js_err(e)))?;
        Ok(Self { db })
    }

    /// Execute one or more statements that return no rows (DDL / pragmas).
    pub fn exec(&self, sql: &str) -> Result<(), SqliteWasmError> {
        raw::exec(&self.db, sql).map_err(|e| SqliteWasmError::Exec(js_err(e)))
    }

    /// Compile `sql` into a prepared statement bound to this connection.
    ///
    /// The returned [`SqliteStmt`] borrows `self`, so it cannot outlive the
    /// connection that owns the underlying handle.
    pub fn prepare(&self, sql: &str) -> Result<SqliteStmt<'_>, SqliteWasmError> {
        let stmt = raw::prepare(&self.db, sql).map_err(|e| SqliteWasmError::Prepare(js_err(e)))?;
        Ok(SqliteStmt {
            stmt,
            _conn: PhantomData,
        })
    }
}

impl Drop for SqliteConn {
    fn drop(&mut self) {
        // Best-effort close; a failure here cannot be propagated from `drop`.
        let _ = raw::close_db(&self.db);
    }
}

/// A prepared statement borrowed from a [`SqliteConn`].
///
/// Parameter indices are **1-based** (SQLite convention); result column indices
/// are **0-based**. The statement is finalized on drop.
pub struct SqliteStmt<'conn> {
    stmt: JsValue,
    _conn: PhantomData<&'conn SqliteConn>,
}

impl SqliteStmt<'_> {
    /// Bind a UTF-8 text value to the 1-based parameter `idx`.
    pub fn bind_text(&self, idx: i32, value: &str) -> Result<(), SqliteWasmError> {
        raw::bind_text(&self.stmt, idx, value).map_err(|e| SqliteWasmError::Bind(js_err(e)))
    }

    /// Bind a byte blob to the 1-based parameter `idx`.
    pub fn bind_blob(&self, idx: i32, value: &[u8]) -> Result<(), SqliteWasmError> {
        raw::bind_blob(&self.stmt, idx, value).map_err(|e| SqliteWasmError::Bind(js_err(e)))
    }

    /// Bind a 64-bit integer to the 1-based parameter `idx`.
    pub fn bind_int64(&self, idx: i32, value: i64) -> Result<(), SqliteWasmError> {
        raw::bind_int64(&self.stmt, idx, value).map_err(|e| SqliteWasmError::Bind(js_err(e)))
    }

    /// Advance the statement one step.
    ///
    /// Returns `true` if a result row is available (SQLITE_ROW) and `false` when
    /// the statement is done (SQLITE_DONE).
    pub fn step(&self) -> Result<bool, SqliteWasmError> {
        raw::step(&self.stmt).map_err(|e| SqliteWasmError::Step(js_err(e)))
    }

    /// Read the 0-based result column `idx` as UTF-8 text.
    pub fn column_text(&self, idx: i32) -> Result<String, SqliteWasmError> {
        raw::column_text(&self.stmt, idx).map_err(|e| SqliteWasmError::Column(js_err(e)))
    }

    /// Read the 0-based result column `idx` as an owned byte blob.
    pub fn column_blob(&self, idx: i32) -> Result<Vec<u8>, SqliteWasmError> {
        raw::column_blob(&self.stmt, idx).map_err(|e| SqliteWasmError::Column(js_err(e)))
    }

    /// Read the 0-based result column `idx` as a 64-bit integer.
    pub fn column_int64(&self, idx: i32) -> Result<i64, SqliteWasmError> {
        raw::column_int64(&self.stmt, idx).map_err(|e| SqliteWasmError::Column(js_err(e)))
    }

    /// Reset the statement for re-execution (also clears bindings).
    pub fn reset(&self) -> Result<(), SqliteWasmError> {
        raw::reset(&self.stmt).map_err(|e| SqliteWasmError::Step(js_err(e)))
    }
}

impl Drop for SqliteStmt<'_> {
    fn drop(&mut self) {
        // Best-effort finalize; a failure here cannot be propagated from `drop`.
        let _ = raw::finalize(&self.stmt);
    }
}
