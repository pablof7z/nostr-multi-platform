//! Native-only helpers for [`super::WasmRuntime`].
//!
//! Wasm32 drives relay events through `BrowserRelayDriver` callbacks.
//! Native protocol-conformance tests call these synchronous helpers to
//! exercise the same `KernelReducer` relay-observation path without spinning
//! up a real WebSocket. Included into `runtime.rs` via `#[path]` when
//! `not(target_arch = "wasm32")`.

impl super::WasmRuntime {
    /// Native test-side shim — the wasm-bindgen `NmpWasmRuntime` only
    /// exposes the `wasm32` method, but the protocol-conformance tests run
    /// on native CI and need a no-op equivalent so the test target compiles
    /// without `#[cfg]` fences in every fixture.
    pub fn set_snapshot_callback(&mut self, _callback: Option<()>) {}

    /// Inject a relay-connected event into the kernel (native test helper).
    pub fn inject_relay_connected_for_test(
        &mut self,
        role: nmp_core::RelayRole,
        url: &str,
    ) {
        let _ = self.reducer.borrow_mut().handle_relay_connected(role, url, false);
    }

    /// Inject a relay text frame into the kernel (native test helper).
    pub fn inject_relay_text_frame_for_test(
        &mut self,
        role: nmp_core::RelayRole,
        url: &str,
        text: String,
    ) {
        let _ = self
            .reducer
            .borrow_mut()
            .handle_relay_frame(role, url, nmp_core::RelayFrame::Text(text));
    }

    /// Pull a snapshot as raw FlatBuffers bytes (native test helper). On
    /// wasm32 every kernel mutation pushes a snapshot through the JS callback;
    /// native tests pull explicitly via this method.
    pub fn snapshot_bytes_for_test(&mut self) -> Vec<u8> {
        use crate::snapshot::build_snapshot_bytes;
        build_snapshot_bytes(&mut self.reducer.borrow_mut(), &self.meta.borrow())
    }
}
