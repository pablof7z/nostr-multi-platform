//! wasm-bindgen / OPFS JavaScript interop shim (wasm32 only).
//!
//! PR-1 stub. The OPFS handle acquisition, SQLite VFS bridge, and Worker
//! message glue land in PR-2/PR-3 (#1007). For now this module only anchors
//! the wasm-only dependencies (`wasm-bindgen`, `js-sys`, `web-sys`) into the
//! build so the dependency wiring is exercised on the wasm32 target.

use js_sys as _;
use wasm_bindgen as _;
use web_sys as _;
