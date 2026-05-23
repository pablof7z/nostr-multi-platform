//! Browser-facing surface for NMP.
//!
//! This crate keeps the wire contract host-testable while the browser actor
//! driver grows behind it. Web hosts render snapshots and execute capabilities;
//! Rust remains responsible for policy and state transitions.

pub mod protocol;
// V-01 Stage 3 — `BrowserRelayDriver`, the wasm32 transport that closes the
// gap the Stage 2 `KernelReducer` left open. Gated to `wasm32` because it
// depends on `web_sys::WebSocket`; building on native targets would need a
// polyfill that adds no value (the native crate already owns the native
// transport).
#[cfg(target_arch = "wasm32")]
pub mod relay_driver;
// V-01 Stage 3 — runtime-side pool helpers: spawn one driver per bootstrap
// entry, build the shared outbound sink, tear them all down on Stop.
// `pub(crate)` because nothing outside the crate constructs a relay pool
// directly — the runtime owns the lifecycle.
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

    use crate::{protocol::WorkerRequest, runtime::WasmRuntime};

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
    }

    impl Default for NmpWasmRuntime {
        fn default() -> Self {
            Self::new()
        }
    }
}
