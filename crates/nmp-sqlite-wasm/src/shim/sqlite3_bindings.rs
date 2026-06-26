//! Raw `wasm-bindgen` extern bindings over the vendored JS shim glue
//! (`vendor/sqlite-wasm/nmp-sqlite3-shim.mjs`), wasm32 only.
//!
//! This module is the *unsafe-ish boundary*: every binding returns a `JsValue`
//! (opaque handle) or a `Result<_, JsValue>` (the `catch` variants surface a
//! thrown JS exception as `Err`). Nothing here is a safe API — that is built on
//! top in [`super`]. The two async entry points (`init`, `installPoolVfs`)
//! return `js_sys::Promise`; callers await them through `JsFuture`.
//!
//! The `module` path is resolved by the bundler at `wasm-bindgen` time (PR-6's
//! conformance vehicle), not during `cargo check`: the proc-macro only records
//! the import string, so compile-checking the shim needs no JS toolchain.

use js_sys::Promise;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/vendor/sqlite-wasm/nmp-sqlite3-shim.mjs")]
extern "C" {
    /// Initialise the SQLite WASM module once (idempotent). Async.
    #[wasm_bindgen(js_name = "init")]
    pub(super) fn init() -> Promise;

    /// Install + register the opfs-sahpool VFS once (idempotent). Async.
    #[wasm_bindgen(js_name = "installPoolVfs")]
    pub(super) fn install_pool_vfs() -> Promise;

    /// Open (creating if absent) a database on the opfs-sahpool VFS.
    #[wasm_bindgen(js_name = "openDb", catch)]
    pub(super) fn open_db(name: &str) -> Result<JsValue, JsValue>;

    /// Execute statements that return no rows (DDL / pragmas).
    #[wasm_bindgen(js_name = "exec", catch)]
    pub(super) fn exec(db: &JsValue, sql: &str) -> Result<(), JsValue>;

    /// Close a database handle.
    #[wasm_bindgen(js_name = "closeDb", catch)]
    pub(super) fn close_db(db: &JsValue) -> Result<(), JsValue>;

    /// Compile one SQL statement into a prepared statement handle.
    #[wasm_bindgen(js_name = "prepare", catch)]
    pub(super) fn prepare(db: &JsValue, sql: &str) -> Result<JsValue, JsValue>;

    /// Bind a UTF-8 text value to a 1-based parameter.
    #[wasm_bindgen(js_name = "bindText", catch)]
    pub(super) fn bind_text(stmt: &JsValue, idx: i32, value: &str) -> Result<(), JsValue>;

    /// Bind a byte blob to a 1-based parameter.
    #[wasm_bindgen(js_name = "bindBlob", catch)]
    pub(super) fn bind_blob(stmt: &JsValue, idx: i32, value: &[u8]) -> Result<(), JsValue>;

    /// Bind a 64-bit integer (marshalled to a JS BigInt) to a 1-based parameter.
    #[wasm_bindgen(js_name = "bindInt64", catch)]
    pub(super) fn bind_int64(stmt: &JsValue, idx: i32, value: i64) -> Result<(), JsValue>;

    /// Advance one step; `true` means a row is available (SQLITE_ROW).
    #[wasm_bindgen(js_name = "step", catch)]
    pub(super) fn step(stmt: &JsValue) -> Result<bool, JsValue>;

    /// Read a 0-based column as UTF-8 text.
    #[wasm_bindgen(js_name = "columnText", catch)]
    pub(super) fn column_text(stmt: &JsValue, idx: i32) -> Result<String, JsValue>;

    /// Read a 0-based column as an owned byte blob.
    #[wasm_bindgen(js_name = "columnBlob", catch)]
    pub(super) fn column_blob(stmt: &JsValue, idx: i32) -> Result<Vec<u8>, JsValue>;

    /// Read a 0-based column as a 64-bit integer (BigInt → i64).
    #[wasm_bindgen(js_name = "columnInt64", catch)]
    pub(super) fn column_int64(stmt: &JsValue, idx: i32) -> Result<i64, JsValue>;

    /// Reset a statement for re-execution (also clears bindings).
    #[wasm_bindgen(js_name = "reset", catch)]
    pub(super) fn reset(stmt: &JsValue) -> Result<(), JsValue>;

    /// Finalize (free) a prepared statement.
    #[wasm_bindgen(js_name = "finalize", catch)]
    pub(super) fn finalize(stmt: &JsValue) -> Result<(), JsValue>;
}
