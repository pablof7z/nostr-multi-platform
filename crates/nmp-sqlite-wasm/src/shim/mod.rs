//! Public shim interface to the JS sqlite3 WASM module.
//!
//! This module provides the JavaScript bindings to instantiate and interact with
//! the vendored sqlite3.wasm module via OPFS SyncAccessHandle pool VFS.

pub mod sqlite3_bindings;

// Re-exports will be added when Slice A fills the module
