//! Browser-facing surface for NMP.
//!
//! This crate keeps the wire contract host-testable while the browser actor
//! driver grows behind it. Web hosts render snapshots and execute capabilities;
//! Rust remains responsible for policy and state transitions.

pub mod protocol;
// V-01 Stage 3 — `BrowserRelayDriver`, the wasm32 transport that closes the
// gap the Stage 2 `KernelReducer` left open. Step 8 phase C moved the
// driver itself into `nmp_network::browser_driver` so both transports live
// in `nmp-network`; this crate now consumes it (constructing the
// `BrowserKernelHandlers` callback bag from its `KernelReducer` handle in
// `relay_pool::build_handlers`).
// V-01 Stage 3 — runtime-side pool helpers: spawn one driver per bootstrap
// entry, build the kernel-handler callback bag + outbound sink, tear them
// all down on Stop. `pub(crate)` because nothing outside the crate
// constructs a relay pool directly — the runtime owns the lifecycle.
#[cfg(target_arch = "wasm32")]
mod relay_pool;
mod runtime;
// V-01 Stage 3b — signer install path + snapshot push helpers. Both modules
// are always-compiled (no `cfg(wasm32)`): the signer slot is a `Signer`
// trait object usable on any target (Nip07Signer.sign() returns Unsupported
// off-wasm, which is the same honest answer the runtime would give anyway).
// snapshot.rs builds the envelope on both targets; the JS-callback push
// inside it is `cfg(target_arch = "wasm32")`-gated, with a native no-op
// shim so call sites stay shim-free.
mod dispatch_routing;
// V-01 Stage 3c — async publish path for app-level write actions on wasm32.
// Always-compiled (the pure reason-string helpers are needed on the native
// `runtime.rs` write-path failure arms too); the `publish_app_action` async
// function and `fan_out_outbound` helper are `cfg(target_arch = "wasm32")`-
// gated because they own `BrowserRelayDriver` and `js_sys::Function`
// references — neither exists off-wasm.
mod publish_path;
mod signer_slot;
mod snapshot;

pub use protocol::{
    ActionDispatch, AppAction, AppActionDispatch, CapabilityFailure, CapabilityResult,
    ClientHello, DegradedMode, RelayBootstrapEntry, RuntimeStatus, SetSigner, StartConfig,
    WorkerEvent, WorkerRequest,
};
pub use runtime::{WasmRuntime, WasmRuntimeError};

#[cfg(target_arch = "wasm32")]
mod bindings {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::future_to_promise;

    use crate::{
        protocol::{AppActionDispatch, WorkerRequest},
        runtime::WasmRuntime,
    };

    #[wasm_bindgen]
    pub struct NmpWasmRuntime {
        runtime: WasmRuntime,
    }

    #[wasm_bindgen]
    impl NmpWasmRuntime {
        #[wasm_bindgen(constructor)]
        pub fn new() -> Self {
            Self {
                runtime: WasmRuntime::new(),
            }
        }

        pub fn handle_json(&mut self, request: &str) -> Result<JsValue, JsValue> {
            let request: WorkerRequest =
                serde_json::from_str(request).map_err(|err| JsValue::from_str(&err.to_string()))?;
            let event = self
                .runtime
                .handle(request)
                .map_err(|err| JsValue::from_str(&err.to_string()))?;
            Ok(JsValue::from_str(
                &serde_json::to_string(&event)
                    .map_err(|err| JsValue::from_str(&err.to_string()))?,
            ))
        }

        /// V-01 Stage 3b — install a JS callback the runtime invokes whenever
        /// a relay-driven kernel mutation produces a fresh snapshot.
        ///
        /// The callback receives one string argument: the JSON-serialized
        /// `WorkerEvent::Update` envelope (`{"type":"update","envelope":{…}}`)
        /// with the same `v` payload `handle_json("start")` returns. JS hosts
        /// install one callback at app boot; replacing the callback (calling
        /// `set_snapshot_callback` again) atomically swaps it.
        ///
        /// Pass `null` (or omit the callback entirely on the JS side) to clear
        /// the slot — the runtime then falls back to pull-only mode.
        ///
        /// # No polling
        ///
        /// The push is driven by the relay-pool sink, which fires only when
        /// a `WebSocket::onmessage` callback delivers an inbound frame. No
        /// periodic timer is scheduled here.
        #[wasm_bindgen]
        pub fn set_snapshot_callback(&mut self, callback: Option<js_sys::Function>) {
            self.runtime.set_snapshot_callback(callback);
        }

        /// V-51 phase 2 — JSON snapshot of the kernel's recent routing
        /// decisions (the bounded `RoutingTraceProjection` ring buffer).
        ///
        /// Mirrors the iOS FFI symbol `nmp_app_recent_routing_decisions`:
        /// same payload shape, schema-versioned (`schema_version: 1`), so
        /// the web Chirp shell can share the routing-inspector renderer
        /// V-51 phase 3 paints over both surfaces.
        ///
        /// Pull-only — call this on demand (long-press / "show routing
        /// trace" toggle); the runtime does not push it on every snapshot
        /// tick. Always returns a well-formed document — empty rings
        /// render as `{"schema_version":1,"capacity":0,"publishes":[],
        /// "subscriptions":[]}` rather than `null` (D6).
        #[wasm_bindgen]
        pub fn recent_routing_decisions(&self) -> String {
            self.runtime.recent_routing_decisions()
        }

        /// V-01 Stage 3c — async dispatch entrypoint for app-level write
        /// actions that need an installed signer.
        ///
        /// `request_json` is a JSON-serialized [`AppActionDispatch`] — same
        /// payload shape `handle_json` accepts inside the
        /// `{"type":"chirp_action","action":…,"correlation_id":…}` envelope,
        /// but unwrapped to the inner dispatch. (The host already knows it's
        /// dispatching an app action when it calls this method, so the
        /// `"type":"chirp_action"` discriminator is redundant.)
        ///
        /// Returns a `js_sys::Promise` resolving to the JSON-serialized
        /// [`WorkerEvent`] — either `ActionAccepted` on a successful sign +
        /// publish, or `CapabilityFailure` for every honest failure mode
        /// (no signer, wrong backend, action variant not yet wired, sign
        /// rejected, sign failed). The Promise rejects only on invalid
        /// `request_json` (deserialisation failure) — the JS host should
        /// treat a rejection as a programmer error, not a runtime failure
        /// to surface to the user.
        ///
        /// # Why a separate entrypoint
        ///
        /// `handle_json` is synchronous (`-> Result<JsValue, JsValue>`) so
        /// the JS host gets the result on the same call. The write path
        /// requires awaiting `window.nostr.signEvent(...)` — a JS Promise
        /// the wasm thread cannot block on. Exposing the async path as a
        /// separate method keeps the synchronous `handle_json` shape
        /// unchanged (kernel-namespaced dispatches, Start, Stop, SetSigner,
        /// and read-side traffic stay fast) while routing the only path
        /// that needs an await through a Promise the host can `await`
        /// directly.
        ///
        /// # Doctrine
        ///
        /// - **D6**: every failure mode surfaces as a `CapabilityFailure`
        ///   inside the resolved Promise — never a Promise rejection on
        ///   anything the user can cause (signer not installed, denial,
        ///   relay timeout). Rejection is reserved for caller bugs.
        /// - **D8**: the only `.await` is `JsFuture::from(promise).await`
        ///   inside `sign_event_via_extension`, which yields to the JS
        ///   event loop in the standard wasm-bindgen-futures way. No
        ///   `try_recv` busy-loop, no `recv_timeout` blocking.
        #[wasm_bindgen]
        pub fn dispatch_app_action_async(&mut self, request_json: &str) -> js_sys::Promise {
            let parsed: Result<AppActionDispatch, _> = serde_json::from_str(request_json);
            let dispatch = match parsed {
                Ok(d) => d,
                Err(err) => {
                    let message = format!("dispatch_app_action_async: invalid request_json: {err}");
                    return js_sys::Promise::reject(&JsValue::from_str(&message));
                }
            };
            // Source `created_at` from `Date.now()` — the kernel's own
            // FixedClock-aware path is `pub(crate)` and not reachable through
            // `KernelReducer`. Production callers never rely on a kernel
            // clock for publish timestamps; tests on wasm32 are TBD (this PR
            // does not add `wasm-bindgen-test` infrastructure).
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let now_secs = (js_sys::Date::now() / 1000.0) as u64;
            let future = self.runtime.start_publish_app_action(
                dispatch.action,
                dispatch.correlation_id,
                now_secs,
            );
            future_to_promise(async move {
                let event = future.await;
                serde_json::to_string(&event)
                    .map(|s| JsValue::from_str(&s))
                    .map_err(|err| JsValue::from_str(&err.to_string()))
            })
        }
    }

    impl Default for NmpWasmRuntime {
        fn default() -> Self {
            Self::new()
        }
    }
}
