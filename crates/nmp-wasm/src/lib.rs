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

pub use protocol::{
    ActionDispatch, AppAction, AppActionDispatch, CapabilityFailure, CapabilityResult,
    ClientHello, DegradedMode, RelayBootstrapEntry, RuntimeStatus, StartConfig, WorkerEvent,
    WorkerRequest,
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
    }

    impl Default for NmpWasmRuntime {
        fn default() -> Self {
            Self::new()
        }
    }
}
