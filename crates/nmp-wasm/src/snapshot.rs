//! Snapshot envelope construction and (wasm32) callback push.
//!
//! Split out of `runtime.rs` so the relay-pool sink can build and push a
//! snapshot directly from its outbound-fanout closure — no detour back through
//! `RawWasmAbiAdapter` (which it doesn't own, and which is
//! `!Send`-by-design because the wasm runtime is single-threaded under the JS
//! event loop).
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
//! No app nouns. The FlatBuffers frame is kernel-authored: `make_update_frame`
//! builds the complete Tier-3 envelope (relay statuses, wire subs, metrics,
//! error toasts) **plus** the Tier-2 typed-projection sidecar (configured
//! relays, publish queue, …). The snapshot payload carries only
//! protocol-neutral fields.
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

use nmp_core::{KernelReducer, ProjectionMergeCache};

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
    /// `Start` flips this to `true`; `Stop` flips it back. Forwarded to
    /// `make_update_frame(running)` so the kernel encodes the correct
    /// running state in the Tier-3 envelope.
    pub(crate) started: bool,
    /// Database name captured at `Start` time. Echoed through the snapshot
    /// so hosts can verify the start handshake. ADR-0054 Stage #5 adds the
    /// boot-time event-store injection seam; the OPFS-SQLite backend is the
    /// follow-up that will make this name select a durable browser store.
    pub(crate) database_name: String,
    /// Relay bootstrap captured at `Start` time. Used to seed the kernel's
    /// configured-relay lanes (via `set_configured_relays`) and to spawn
    /// the `BrowserRelayDriver` instances on wasm32. Cleared on a fresh
    /// runtime before `Start`.
    pub(crate) relay_bootstrap: Vec<crate::protocol::RelayBootstrapEntry>,
    /// Optional post-encode frame observer (issue #1767). Installed by a
    /// composition root via the raw ABI adapter's frame observer and
    /// fired from [`build_snapshot_bytes`] with the just-encoded frame bytes —
    /// AFTER `make_update_frame` returns, so the callback sees only `&[u8]`
    /// and never re-enters the reducer. This is the wasm twin of nmp-ffi's
    /// listener-thread `update_embed_sidecar_from_frame` hook: a composition
    /// root that depends on `nmp-content` decodes the `claimed_events` KCEV
    /// from the bytes, resolves each embed, and stores the resolved map in its
    /// own slot, which a typed snapshot projection reads on the NEXT tick
    /// (one-tick lag — identical to native, and acceptable for the async
    /// claimed-events flow). `nmp-wasm` itself stays policy-free: it owns the
    /// chokepoint, not the resolution.
    ///
    /// `Rc<dyn Fn(&[u8])>` (not `Send + Sync`) is correct on wasm32: the JS
    /// event loop is single-threaded, and the callback runs synchronously on
    /// the same thread that built the frame.
    pub(crate) frame_observer: Option<Rc<dyn Fn(&[u8])>>,
    /// Rust-owned projection merge cache. Every outbound snapshot frame passes
    /// through this before crossing to JS, so TS renders a current full sidecar
    /// frame and never owns Changed/Cleared/absent retention policy.
    pub(crate) projection_merge_cache: ProjectionMergeCache,
}

impl RuntimeMeta {
    pub(crate) fn new() -> Self {
        Self {
            started: false,
            database_name: String::new(),
            relay_bootstrap: Vec::new(),
            frame_observer: None,
            projection_merge_cache: ProjectionMergeCache::default(),
        }
    }
}

/// Encode the current kernel state as one FlatBuffers update frame.
///
/// Delegates entirely to [`KernelReducer::make_update_frame`], which builds
/// the complete Tier-3 envelope (relay statuses from real `RelayHealth` data,
/// wire subscriptions, metrics, error toasts) **and** the Tier-2
/// typed-projection sidecar (configured relays, publish queue, …). The kernel
/// is the sole author of the revision counter (`rev` bumps monotonically inside
/// `make_update`), so the host's monotonic-rev guard continues to work unchanged.
///
/// `meta.started` is forwarded as the `running` flag so the envelope reflects
/// the current lifecycle state.
pub(crate) fn build_snapshot_bytes(reducer: &mut KernelReducer, meta: &mut RuntimeMeta) -> Vec<u8> {
    let bytes = reducer.make_update_frame(meta.started);
    let bytes = meta
        .projection_merge_cache
        .merge_update_frame(&bytes)
        .unwrap_or(bytes);
    // #1767 — fire the post-encode frame observer (if a composition root
    // installed one) AFTER the bytes are produced. The callback receives only
    // `&[u8]`; it must not touch the reducer (which is mutably borrowed by this
    // call's caller). This is the single chokepoint for EVERY snapshot path
    // (tick, relay-push, handle-return, publish) so one hook covers them all.
    if let Some(observer) = meta.frame_observer.as_ref() {
        observer(&bytes);
    }
    bytes
}

/// Push a snapshot envelope through the JS callback the host registered via
/// `NmpWasmRuntime::set_snapshot_callback`, if any. Called from the relay
/// pool's sink after every kernel-mutating inbound frame.
///
/// `wasm32`-only: native targets don't own a `js_sys::Function`. The native
/// path uses the synchronous return value of `RawWasmAbiAdapter::handle` instead;
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
    let bytes = build_snapshot_bytes(&mut reducer.borrow_mut(), &mut meta.borrow_mut());
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
