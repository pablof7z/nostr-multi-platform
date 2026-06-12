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

    /// Simulate one tick cycle (native test helper). Returns `(outbound,
    /// dirty)` where `dirty` mirrors `KernelReducer::changed_since_emit()`
    /// **after** the tick — the same flag the wasm32 timer checks before
    /// pushing a snapshot.
    ///
    /// Calling this helper exercises the identical coalescing path as the
    /// wasm32 `gloo-timers` closure: both call [`crate::tick::tick_once`],
    /// so a native test asserting `dirty == false` proves the timer would
    /// not push a spurious snapshot.
    pub fn tick_for_test(&mut self) -> (Vec<nmp_core::OutboundMessage>, bool) {
        crate::tick::tick_once(&self.reducer)
    }

    /// Read the active-account pubkey the kernel currently holds (native test
    /// helper). Returns the canonical lowercase hex string, or `None` if no
    /// account is active.
    pub fn active_account_pubkey_for_test(&self) -> Option<String> {
        self.reducer.borrow().active_account_pubkey()
    }
}

#[cfg(test)]
mod set_signer_tests {
    //! B2 — canonicalization guard: `set_signer` with an uppercase pubkey hex
    //! must store a canonical (lowercase) active account so contact-feed REQs
    //! carry the correct key in their author filters.
    use super::super::WasmRuntime;
    use crate::protocol::{SetSigner, WorkerRequest};

    const LOWER_PK: &str =
        "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    const UPPER_PK: &str =
        "3BF0C63FCB93463407AF97A5E5EE64FA883D107EF9E558472C4EB9AAAEFA459D";

    #[test]
    fn set_signer_uppercase_pubkey_stores_canonical_lowercase_active_account() {
        // B2: raw uppercase pubkey on the wire must be normalised to lowercase
        // before being stored as active_account. Without the fix the kernel
        // holds an uppercase active_account key, and contact-feed REQs carry
        // an uppercase `authors` filter — breaking NIP-01 relay compliance.
        let mut runtime = WasmRuntime::new();
        let result = runtime
            .handle(WorkerRequest::SetSigner(SetSigner {
                kind: "nip07".to_string(),
                pubkey_hex: UPPER_PK.to_string(),
                correlation_id: "set-signer-b2".to_string(),
            }))
            .expect("set_signer must succeed");

        // The handle call must succeed (ActionAccepted + snapshot, not an error).
        assert!(
            result.iter().any(|e| matches!(
                e,
                crate::protocol::WorkerEvent::ActionAccepted { .. }
            )),
            "set_signer with valid uppercase pubkey must return ActionAccepted; got: {result:?}"
        );

        // The kernel must store the lowercase canonical form.
        assert_eq!(
            runtime.active_account_pubkey_for_test().as_deref(),
            Some(LOWER_PK),
            "active_account must be lowercase canonical hex even when input is uppercase"
        );
    }
}
