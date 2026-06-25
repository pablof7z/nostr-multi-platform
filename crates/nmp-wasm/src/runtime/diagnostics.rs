//! Pull-only diagnostics for [`super::RawWasmAbiAdapter`]. Internal API.

use super::RawWasmAbiAdapter;

impl RawWasmAbiAdapter {
    /// V-51 phase 2 - JSON snapshot of the kernel's recent routing decisions.
    /// Internal API.
    ///
    /// Sibling of the FFI `nmp_app_recent_routing_decisions` symbol; same
    /// payload shape on both surfaces so the web Chirp shell and the iOS Chirp
    /// shell can share a single routing-inspector renderer (V-51 phase 3).
    ///
    /// Pull-only: the runtime does not push this on every snapshot tick.
    /// Routing traces are diagnostic; the cost model is "pay when a host asks".
    #[must_use]
    pub(crate) fn recent_routing_decisions(&self) -> String {
        self.reducer.borrow().recent_routing_decisions_json()
    }
}
