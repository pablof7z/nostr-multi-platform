//! `NmpWasmRuntime` — wasm-bindgen entry point for `nmp-browser-runtime`
//! (#2038 item A).
//!
//! This module satisfies the TypeScript contract in
//! `web/packages/runtime-web/src/wasmBridge.ts`:
//!
//! ```ts
//! type NmpWasmRuntime = {
//!   handle_json(request: string): unknown;
//!   handle_dispatch_bytes?(bytes: Uint8Array): unknown;
//!   recent_routing_decisions?(): string;
//!   set_snapshot_callback?(callback: SnapshotCallback | null): void;
//! };
//! type NmpWasmModule = {
//!   NmpWasmRuntime?: new () => NmpWasmRuntime;
//!   nmp_encode_npub?: EncodeNpubFn;
//! };
//! ```
//!
//! # Snapshot delivery
//!
//! Snapshots (FlatBuffers merged update-frame bytes) are pushed asynchronously
//! via the callback registered through [`NmpWasmRuntime::set_snapshot_callback`].
//! They are NOT returned inline from `handle_json`; the bridge code in
//! `wasmBridge.ts` sets the callback in its constructor before sending any
//! requests.
//!
//! # Async pump loop
//!
//! On `wasm32` the runtime arms a one-shot `gloo_timers` callback set to 0ms
//! after inbound relay events arrive (via the `set_wake` hook). The callback
//! calls `pump_once()` which drains the inbox, runs the maintenance tick, and
//! fans outbound to relay drivers — then pushes the snapshot.
//!
//! On native (CI / tests) there is no JS runtime so the pump is driven
//! manually by tests or by `NmpRuntimeCore::pump_once`.
//!
//! # Always-compiled non-wasm code
//!
//! `NmpRuntimeCore` (in `super::core`) and all protocol types (in
//! `super::protocol`) are always-compiled. This lets `cargo test -p
//! nmp-browser-runtime` exercise the full routing logic without a wasm target.
//! Only the `#[wasm_bindgen]` wrappers and JS/gloo imports are gated on
//! `cfg(target_arch = "wasm32")`.

pub(crate) mod core;
pub(crate) mod dispatch;
pub(crate) mod identity;
pub(crate) mod protocol;
pub(crate) mod ref_routing;

// ── wasm32 entry point ────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use std::cell::RefCell;
    use std::rc::Rc;

    use js_sys::{Function, Uint8Array};
    use wasm_bindgen::prelude::*;

    use super::core::NmpRuntimeCore;

    // ── Shared-state wrapper ─────────────────────────────────────────────────

    /// Internal shared state, wrapped in `Rc<RefCell<>>` so closures (wake timer,
    /// snapshot sink) can share ownership with `NmpWasmRuntime` without crossing
    /// thread boundaries. Single-threaded wasm32 guarantees no contention.
    struct Inner {
        core: NmpRuntimeCore,
        /// JS function to call with `(bytes: Uint8Array)` on snapshot push.
        snapshot_cb: Option<Function>,
    }

    impl Inner {
        fn new() -> Self {
            Self {
                core: NmpRuntimeCore::new(),
                snapshot_cb: None,
            }
        }

        /// Push snapshot bytes via the installed JS callback, if any.
        fn push_snapshot_via_js(&mut self) {
            if self.snapshot_cb.is_none() {
                return;
            }
            if let Some(h) = self.core.handle.as_mut() {
                if let Some(bytes) = h.produce_snapshot_bytes(true) {
                    if let Some(cb) = &self.snapshot_cb {
                        let arr = js_sys::Uint8Array::from(bytes.as_slice());
                        // D6 — drop the error; a broken callback is not fatal.
                        let _ = cb.call1(&JsValue::NULL, &arr);
                    }
                }
            }
        }

        /// Drive one pump turn and push the snapshot if data arrived.
        fn pump_and_push_snapshot(&mut self) {
            let events = self.core.pump_once();
            // Sign-request events from async pump turns (relay-driven wakes) need
            // to reach the host. For now there is no deferred event buffer; those
            // events surface synchronously from handle_json only. The pump here is
            // relay-inbound-only (no new sign round-trips are started mid-turn).
            // Future: buffer in `pending_host_events` and deliver on next handle_json.
            let _ = events; // consumed; future: post to a JS event queue
            self.push_snapshot_via_js();
        }
    }

    // ── NmpWasmRuntime ─────────────────────────────────────────────────────────

    /// `NmpWasmRuntime` — the browser JS host constructs one instance per Worker,
    /// then drives it with `handle_json` / `handle_dispatch_bytes`.
    ///
    /// Exported to JS as `new NmpWasmRuntime()`.
    #[wasm_bindgen]
    pub struct NmpWasmRuntime {
        inner: Rc<RefCell<Inner>>,
    }

    #[wasm_bindgen]
    impl NmpWasmRuntime {
        /// Construct an unstarted runtime.
        ///
        /// Call `handle_json` with a `WorkerRequest::Hello` then
        /// `WorkerRequest::Start` before any other requests.
        #[wasm_bindgen(constructor)]
        pub fn new() -> NmpWasmRuntime {
            // Install the panic hook once (idempotent).
            crate::install_panic_hook();

            NmpWasmRuntime {
                inner: Rc::new(RefCell::new(Inner::new())),
            }
        }

        /// Handle a JSON-serialised `WorkerRequest` and return a JSON array
        /// of `WorkerEvent`s.
        ///
        /// The return value is `unknown` from the JS side; the bridge casts it
        /// via `parseWorkerEvents`. Large binary payloads should go through
        /// `handle_dispatch_bytes` instead.
        pub fn handle_json(&mut self, request: &str) -> JsValue {
            let result = {
                let mut inner = self.inner.borrow_mut();
                let json = inner.core.handle_json_request(request);
                // Push snapshot after every mutable request.
                inner.push_snapshot_via_js();
                json
            };
            JsValue::from_str(&result)
        }

        /// Handle raw `DispatchEnvelope` bytes (ADR-0064 binary write doorway).
        ///
        /// Avoids JSON round-tripping the binary payload, which would corrupt it.
        /// Returns a JSON array of `WorkerEvent`s (same as `handle_json`).
        pub fn handle_dispatch_bytes(&mut self, bytes: &[u8]) -> JsValue {
            let result = {
                let mut inner = self.inner.borrow_mut();
                let json = inner.core.handle_dispatch_bytes_raw(bytes);
                inner.push_snapshot_via_js();
                json
            };
            JsValue::from_str(&result)
        }

        /// Return a JSON snapshot of recent routing decisions (pull-only,
        /// diagnostic). Does NOT trigger a snapshot push.
        pub fn recent_routing_decisions(&self) -> String {
            self.inner.borrow().core.recent_routing_decisions()
        }

        /// Install (or clear) the snapshot callback.
        ///
        /// The callback is called with a `Uint8Array` of merged FlatBuffers
        /// update-frame bytes whenever the kernel state changes. Pass `null`
        /// to uninstall.
        ///
        /// This wires the async pump wake: when inbound relay events arrive the
        /// relay pool fires a 0ms timer that calls `pump_and_push_snapshot()` on
        /// the shared inner state, which drains inbox + pushes snapshot without
        /// holding the borrow at JS boundary.
        pub fn set_snapshot_callback(&mut self, cb: Option<Function>) {
            let mut inner = self.inner.borrow_mut();
            inner.snapshot_cb = cb;

            // Wire the relay wake hook: arm a 0ms gloo_timers callback that
            // pumps and pushes the snapshot. Uses a weak-Rc so the timer
            // doesn't prevent the runtime from being garbage-collected.
            let inner_rc = Rc::clone(&self.inner);
            let wake = Rc::new(move || {
                // `inner_rc` keeps the `Inner` alive long enough to pump.
                if let Ok(mut guard) = inner_rc.try_borrow_mut() {
                    guard.pump_and_push_snapshot();
                }
            });
            if let Some(h) = inner.core.handle.as_mut() {
                h.set_wake(wake);
            }
        }
    }

    impl Default for NmpWasmRuntime {
        fn default() -> Self {
            Self::new()
        }
    }

    // ── Free function: nmp_encode_npub ────────────────────────────────────────

    /// Encode a 32-byte secp256k1 public key (64 hex chars) as an `npub1…` bech32
    /// string.
    ///
    /// Returns the bech32-encoded npub on success, or an empty string if `hex`
    /// is not valid 64-char lowercase-hex (D6: total on JS boundary — never
    /// throws).
    ///
    /// Exported to JS as `nmp_encode_npub(hex: string): string`.
    #[wasm_bindgen]
    pub fn nmp_encode_npub(hex: &str) -> String {
        nmp_core::nip19::encode_npub(hex).unwrap_or_default()
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_impl::{nmp_encode_npub, NmpWasmRuntime};

// ── Non-wasm stubs (native CI / doc builds) ───────────────────────────────────

/// Encode a 32-byte secp256k1 public key hex as `npub1…` bech32.
///
/// On native this is a plain Rust function (no `#[wasm_bindgen]`); the wasm
/// target exports it via `nmp_encode_npub` above. Returns an empty string on
/// invalid input (D6: total — no panic, no error value on the JS boundary).
#[cfg(not(target_arch = "wasm32"))]
pub fn nmp_encode_npub(hex: &str) -> String {
    nmp_core::nip19::encode_npub(hex).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_npub_valid() {
        let hex = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        let npub = nmp_encode_npub(hex);
        assert!(npub.starts_with("npub1"), "npub={npub}");
    }

    #[test]
    fn encode_npub_invalid_returns_empty() {
        let result = nmp_encode_npub("not-valid");
        assert!(result.is_empty(), "result={result:?}");
    }
}
