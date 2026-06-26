//! The crate-local error type, shared across the wasm shim and the
//! target-agnostic codec / schema logic.
//!
//! `SqliteWasmError` is deliberately **not** `nmp_store::StoreError`: this crate
//! cannot depend on `nmp-store` (that would be a Cargo cycle — see the crate-level
//! docs). The `nmp-store` `EventStore` wrapper (a later PR, at the cycle-free
//! seam) owns the `SqliteWasmError -> StoreError` conversion.
//!
//! The type lives in its own module — **not** in the wasm32-only [`crate::shim`] —
//! because the row codec ([`crate::conv`]) and the DDL/migration logic
//! ([`crate::schema`]) are pure and compile on every target; they return this
//! error too, and the codec's round-trip is unit-tested on native (no wasm,
//! no shim). Only the JS-exception mapping (`shim::js_err`) is wasm-gated.

/// Error returned by the SQLite-on-OPFS engine.
///
/// The wrapper converts these into `nmp_store::StoreError` at the boundary:
/// `ModuleInit` / `VfsInstall` / `Open` / `Close` are backend-i/o failures
/// (`StoreError::Io`), `Exec` / `Prepare` / `Bind` / `Step` are statement faults
/// (`StoreError::Io` or `Corrupt`), and `Column` / `Encoding` are decode faults
/// (`StoreError::Encoding`). The wrapped `String` is the stringified JS exception
/// (engine paths) or codec message (`Encoding`); per D6 it carries no private
/// event content.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Encoding/decoding an event blob (or other row value) failed — a data
    /// fault, distinct from an engine-i/o fault. Produced by [`crate::conv`].
    Encoding(String),
    /// A domain-namespace migration failed: either the on-disk schema is newer
    /// than the requested target (the wrapper maps this to
    /// `StoreError::SchemaTooNew`) or a migration step's `apply` closure returned
    /// an error (`StoreError::MigrationFailed`). The message carries which.
    Migration(String),
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
            Self::Encoding(s) => write!(f, "encoding failed: {s}"),
            Self::Migration(s) => write!(f, "domain migration failed: {s}"),
        }
    }
}

impl core::error::Error for SqliteWasmError {}
