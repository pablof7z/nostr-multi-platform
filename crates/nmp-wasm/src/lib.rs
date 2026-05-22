//! Browser-facing surface for NMP.
//!
//! This crate keeps the wire contract host-testable while the browser actor
//! driver grows behind it. Web hosts render snapshots and execute capabilities;
//! Rust remains responsible for policy and state transitions.

pub mod protocol;
mod runtime;

pub use protocol::{
    ActionDispatch, CapabilityFailure, CapabilityResult, ChirpAction, ChirpActionDispatch,
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
