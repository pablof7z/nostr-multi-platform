//! Pull-only diagnostics for [`super::WasmRuntime`].

use super::WasmRuntime;

impl WasmRuntime {
    /// V-51 phase 2 - JSON snapshot of the kernel's recent routing decisions.
    ///
    /// Sibling of the FFI `nmp_app_recent_routing_decisions` symbol; same
    /// payload shape on both surfaces so the web Chirp shell and the iOS Chirp
    /// shell can share a single routing-inspector renderer (V-51 phase 3).
    ///
    /// Pull-only: the runtime does not push this on every snapshot tick.
    /// Routing traces are diagnostic; the cost model is "pay when a host asks".
    /// The `wasm-bindgen` wrapper exposes this as
    /// `NmpWasmRuntime::recent_routing_decisions()`.
    #[must_use]
    pub fn recent_routing_decisions(&self) -> String {
        self.reducer.borrow().recent_routing_decisions_json()
    }
}
