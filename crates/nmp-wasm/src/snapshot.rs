//! Snapshot envelope construction and (wasm32) callback push.
//!
//! Split out of `runtime.rs` so the relay-pool sink can build and push a
//! snapshot directly from its outbound-fanout closure — no detour back through
//! `WasmRuntime` (which it doesn't own, and which is `!Send`-by-design because
//! the wasm runtime is single-threaded under the JS event loop).
//!
//! # Why a separate file
//!
//! 1. `runtime.rs` is approaching the 500-line ceiling. Extracting the
//!    snapshot-builder keeps both files comfortably under the limit and gives
//!    the relay-driven push path a single owner.
//! 2. The snapshot shape needs to be identical whether the request came in
//!    via `Start` (host pulls the frame from `handle()`'s return value)
//!    or via an inbound relay frame (callback push). Putting the build logic
//!    in one place makes the equivalence syntactic, not aspirational.
//!
//! # Substrate-grade (D0)
//!
//! No app nouns. The FlatBuffers frame mirrors what the native actor emits;
//! the snapshot payload carries only protocol-neutral fields
//! (schema version, kernel rev, started flag, relay diagnostics).
//!
//! # wasm→JS transport
//!
//! On `wasm32`, snapshot bytes cross the JS boundary as a raw
//! `js_sys::Uint8Array` argument to the host-installed callback — never as a
//! JSON-wrapped string. Encoding the FlatBuffers frame as a JSON number
//! array bloats a 4Hz hot-path payload ~3–4× and then forces the host to
//! `JSON.parse` + `new Uint8Array(…)` back out. The typed-array hop keeps
//! the binary transport binary.

use std::cell::RefCell;
use std::rc::Rc;

use nmp_core::{encode_snapshot_value, KernelReducer, SNAPSHOT_SCHEMA_VERSION};
use serde_json::Value;

use crate::protocol::RelayBootstrapEntry;

/// Shared metadata the runtime and the relay-pool sink BOTH read from when
/// building a snapshot envelope.
///
/// `Rc<RefCell<…>>` is the correct shape on wasm32: the JS event loop is
/// single-threaded so there is no `Send` requirement, but the sink closure
/// (registered at `Start` time, captured by JS event handlers) outlives any
/// single borrow of the runtime — hence `Rc` for shared ownership and
/// `RefCell` for the interior mutation `Start`/`Stop`/relay-frame paths
/// need.
///
/// Fields are intentionally `pub(crate)` — the metadata is the runtime's
/// single source of truth for snapshot inputs; the snapshot builder reads
/// them, the runtime mutates them on `Start` / `Stop`.
pub(crate) struct RuntimeMeta {
    /// Mirrors the kernel's own `rev` field (visible through
    /// `KernelUpdate::Started { rev }`). Bumped on every successful
    /// kernel-driven update so hosts can apply the monotonic-revision
    /// guard rule.
    pub(crate) rev: u64,
    /// `Start` flips this to `true`; `Stop` flips it back.
    pub(crate) started: bool,
    /// Relay bootstrap captured at `Start` time. Surfaces on the snapshot as
    /// the `relay_diagnostics` projection so the host can verify the start
    /// handshake. Cleared on a fresh runtime (empty Vec) before `Start`.
    pub(crate) relay_bootstrap: Vec<RelayBootstrapEntry>,
    /// Database name captured at `Start` time. Echoed through the snapshot
    /// so hosts can verify the start handshake. The pure kernel never sees
    /// a database (no IndexedDB binding yet — Stage 3b follow-up).
    pub(crate) database_name: String,
}

impl RuntimeMeta {
    pub(crate) fn new() -> Self {
        Self {
            rev: 0,
            started: false,
            relay_bootstrap: Vec::new(),
            database_name: String::new(),
        }
    }
}

/// Build the test-only JSON view from the kernel + runtime metadata. Runtime
/// hosts consume [`build_snapshot_bytes`] instead.
#[cfg(test)]
pub(crate) fn build_snapshot_value(_reducer: &KernelReducer, meta: &RuntimeMeta) -> Value {
    let snapshot = build_snapshot_payload_value(meta);

    serde_json::json!({
        "t": "snapshot",
        "v": snapshot,
    })
}

pub(crate) fn build_snapshot_bytes(_reducer: &KernelReducer, meta: &RuntimeMeta) -> Vec<u8> {
    let snapshot = build_snapshot_payload_value(meta);
    encode_snapshot_value(snapshot)
}

fn build_snapshot_payload_value(meta: &RuntimeMeta) -> Value {
    serde_json::json!({
        "schema_version": SNAPSHOT_SCHEMA_VERSION,
        "rev": meta.rev,
        "running": meta.started,
        "database_name": meta.database_name,
        "projections": {
            "relay_diagnostics": meta.relay_bootstrap.iter().map(|relay| {
                serde_json::json!({
                    "url": relay.url,
                    "role": relay.role,
                    // "configured" is the only status the wasm runtime can
                    // honestly claim until Stage 3b wires per-relay
                    // connection-state observation through the kernel's
                    // `RelayHealth` snapshot projection. The native runtime
                    // surfaces "connected" / "degraded" / "permanent_failure"
                    // here once the equivalent observer is exposed.
                    "status": "configured",
                })
            }).collect::<Vec<_>>()
        }
    })
}

/// Push a snapshot envelope through the JS callback the host registered via
/// `NmpWasmRuntime::set_snapshot_callback`, if any. Called from the relay
/// pool's sink after every kernel-mutating inbound frame.
///
/// `wasm32`-only: native targets don't own a `js_sys::Function`. The native
/// path uses the synchronous return value of `WasmRuntime::handle` instead;
/// no async push surface exists on native because there's no out-of-band
/// kernel mutation source (the native crate uses its own `relay_worker`).
///
/// The callback receives a raw `Uint8Array` of FlatBuffers update-frame
/// bytes — not a JSON-wrapped string. Encoding the FlatBuffers bytes as a
/// JSON array of decimal numbers undoes the whole point of the binary
/// transport (~3–4× bloat on a hot-path snapshot), so the wasm→JS hop
/// uses a typed-array argument and the JS host pushes the resulting
/// `update_bytes` event upstream itself.
///
/// Errors from `Function::call1` are intentionally swallowed: a JS handler
/// throwing should not crash the wasm runtime; the JS side gets the throw
/// at the call site and can log/report. Dropping the frame is honest — the
/// next inbound will re-push a fresh snapshot.
#[cfg(target_arch = "wasm32")]
pub(crate) fn push_snapshot_if_callback(
    callback: &Rc<RefCell<Option<js_sys::Function>>>,
    reducer: &Rc<RefCell<KernelReducer>>,
    meta: &Rc<RefCell<RuntimeMeta>>,
) {
    let bytes = build_snapshot_bytes(&reducer.borrow(), &meta.borrow());
    push_bytes_if_callback(callback, &bytes);
}

/// Inner primitive shared by every wasm→JS snapshot-callback push site
/// (`push_snapshot_if_callback` above for the relay-pool sink and the
/// publish-path fan-out; the `handle_json` drain in `lib.rs` for the
/// synchronous-return path). Keeps the conversion from `&[u8]` to
/// `js_sys::Uint8Array` in one place so the two call sites cannot drift.
///
/// `copy_from` allocates a fresh `Uint8Array` whose backing buffer is owned
/// by the JS heap — safe to hand to a callback that may stash it (the
/// runtime's `&[u8]` is borrowed from the wasm linear memory and would be
/// invalidated by any subsequent `Vec` growth).
#[cfg(target_arch = "wasm32")]
pub(crate) fn push_bytes_if_callback(
    callback: &Rc<RefCell<Option<js_sys::Function>>>,
    bytes: &[u8],
) {
    let callback_ref = callback.borrow();
    let Some(callback_fn) = callback_ref.as_ref() else {
        return;
    };
    let array = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(bytes);
    let _ = callback_fn.call1(&wasm_bindgen::JsValue::NULL, &array.into());
}

/// Native no-op kept for symmetry with the wasm32 surface. Never invoked
/// from the native target (no JS to call into; the relay-pool sink that
/// would call it is wasm32-only), but cargo's dead-code analyser cannot
/// prove that across the `cfg` boundary — silence the warning so the
/// always-on cross-compile gate stays warning-clean.
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub(crate) fn push_snapshot_if_callback(
    _callback: &Rc<RefCell<Option<()>>>,
    _reducer: &Rc<RefCell<KernelReducer>>,
    _meta: &Rc<RefCell<RuntimeMeta>>,
) {
}
