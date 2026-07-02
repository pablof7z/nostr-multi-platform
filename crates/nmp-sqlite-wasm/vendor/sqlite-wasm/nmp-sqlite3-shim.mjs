// nmp-sqlite3-shim.mjs — hand-authored NMP shim glue (NOT part of the upstream
// sqlite.org artifact; see ../PROVENANCE.md).
//
// This is the JavaScript half of the `nmp-sqlite-wasm` wasm-bindgen shim. It
// imports the vendored, public-domain `sqlite3.mjs` (the official sqlite.org
// WASM/JS build) and re-exports a *flat, stable* async API that the Rust extern
// block in `src/shim/sqlite3_bindings.rs` binds via `#[wasm_bindgen(module =
// …)]`. Keeping the adapter surface flat is what makes the Rust bindings
// possible at all: `sqlite3.mjs` only exports the `sqlite3InitModule` factory,
// so the engine's `capi`/`oo1` entry points are properties of an async-returned
// object, not module exports — they cannot be named in a `module` extern until
// a wrapper like this one hoists them into real exports.
//
// Engine choice (ADR-0072 §1): the OPFS *SyncAccessHandle pool* VFS
// ("opfs-sahpool"). Unlike the older async `opfs` VFS, opfs-sahpool is
// synchronous after a one-time async pool open and does NOT require COOP/COEP
// cross-origin isolation or SharedArrayBuffer — so it works on a plain static
// host. The async `opfs` VFS and its `sqlite3-opfs-async-proxy.js` worker are
// intentionally NOT vendored and never installed here.
//
// All decision logic (lifecycle, error mapping, the safe typed surface) lives
// in Rust. This file is the mechanical adapter only.

import sqlite3InitModule from "./sqlite3.mjs";

// Cached singletons. The store is owned by exactly one Web Worker actor
// (ADR-0072 §1 / ADR-0072 §3), so no locking is needed: init runs at most once.
let sqlite3 = null;
let poolUtil = null;
let initPromise = null;
let vfsPromise = null;

/**
 * Initialise the SQLite WASM module exactly once. Idempotent.
 * @returns {Promise<void>}
 */
export function init() {
  if (!initPromise) {
    initPromise = sqlite3InitModule().then((mod) => {
      sqlite3 = mod;
    });
  }
  return initPromise;
}

/**
 * Install + register the OPFS SyncAccessHandle pool VFS exactly once.
 * Async (returns a Promise) because acquiring the SAH pool touches OPFS.
 * Must be called after {@link init}.
 * @returns {Promise<void>}
 */
export function installPoolVfs() {
  if (!vfsPromise) {
    vfsPromise = sqlite3
      .installOpfsSAHPoolVfs({ name: "opfs-sahpool" })
      .then((util) => {
        poolUtil = util;
      });
  }
  return vfsPromise;
}

/**
 * Open (creating if absent) a database file on the opfs-sahpool VFS.
 * @param {string} name bare database name; stored as `/<name>` in the pool.
 * @returns {object} an oo1 DB handle (opaque to Rust).
 */
export function openDb(name) {
  return new poolUtil.OpfsSAHPoolDb("/" + name);
}

/**
 * Execute one or more SQL statements with no result rows (DDL / pragmas).
 * @param {object} db
 * @param {string} sql
 */
export function exec(db, sql) {
  db.exec(sql);
}

/** Close a database handle. @param {object} db */
export function closeDb(db) {
  db.close();
}

/**
 * Compile one SQL statement into a prepared statement.
 * @param {object} db
 * @param {string} sql
 * @returns {object} an oo1 Stmt handle (opaque to Rust).
 */
export function prepare(db, sql) {
  return db.prepare(sql);
}

/** Bind a UTF-8 text value to a 1-based parameter. */
export function bindText(stmt, idx, value) {
  stmt.bind(idx, value);
}

/** Bind a byte blob (Uint8Array) to a 1-based parameter. */
export function bindBlob(stmt, idx, value) {
  stmt.bind(idx, value);
}

/** Bind a 64-bit integer (passed from Rust as a BigInt) to a 1-based parameter. */
export function bindInt64(stmt, idx, value) {
  stmt.bind(idx, value);
}

/** Bind SQL NULL to a 1-based parameter (oo1 maps a JS `null` to SQLITE_NULL). */
export function bindNull(stmt, idx) {
  stmt.bind(idx, null);
}

/**
 * Advance the statement one step.
 * @returns {boolean} true if a row is available (SQLITE_ROW), false when done.
 */
export function step(stmt) {
  return stmt.step();
}

/** Read a 0-based column as UTF-8 text. @returns {string} */
export function columnText(stmt, idx) {
  return stmt.get(idx, sqlite3.capi.SQLITE_TEXT);
}

/** Read a 0-based column as an owned byte blob. @returns {Uint8Array} */
export function columnBlob(stmt, idx) {
  const v = stmt.get(idx, sqlite3.capi.SQLITE_BLOB);
  // oo1 returns null for SQL NULL; normalise to an empty array for the Rust
  // `Vec<u8>` boundary (NULL/empty distinction is a column-policy concern for a
  // later PR, not the raw shim).
  return v == null ? new Uint8Array(0) : v;
}

/**
 * Read a 0-based column as a 64-bit integer with full fidelity.
 * Uses the C API directly so values are returned as a BigInt rather than being
 * narrowed through a JS double.
 * @returns {bigint}
 */
export function columnInt64(stmt, idx) {
  return sqlite3.capi.sqlite3_column_int64(stmt.pointer, idx);
}

/** Reset a statement for re-execution (also clears bindings). */
export function reset(stmt) {
  stmt.reset(true);
}

/** Finalize (free) a prepared statement. */
export function finalize(stmt) {
  stmt.finalize();
}
