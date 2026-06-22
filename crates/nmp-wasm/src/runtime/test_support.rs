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
mod set_identity_tests {
    //! B2 — canonicalization guard: `set_identity` with an uppercase pubkey hex
    //! must store a canonical (lowercase) active account so active-follows REQs
    //! carry the correct key in their author filters.
    use super::super::WasmRuntime;
    use crate::protocol::{SetIdentity, WorkerRequest};

    const LOWER_PK: &str =
        "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    const UPPER_PK: &str =
        "3BF0C63FCB93463407AF97A5E5EE64FA883D107EF9E558472C4EB9AAAEFA459D";

    #[test]
    fn set_identity_uppercase_pubkey_stores_canonical_lowercase_active_account() {
        // B2: raw uppercase pubkey on the wire must be normalised to lowercase
        // before being stored as active_account. Without the fix the kernel
        // holds an uppercase active_account key, and active-follows REQs carry
        // an uppercase `authors` filter — breaking NIP-01 relay compliance.
        let mut runtime = WasmRuntime::new();
        let result = runtime
            .handle(WorkerRequest::SetIdentity(SetIdentity {
                kind: "nip07".to_string(),
                pubkey_hex: UPPER_PK.to_string(),
                correlation_id: "set-identity-b2".to_string(),
            }))
            .expect("set_identity must succeed");

        // The handle call must succeed (ActionAccepted + snapshot, not an error).
        assert!(
            result.iter().any(|e| matches!(
                e,
                crate::protocol::WorkerEvent::ActionAccepted { .. }
            )),
            "set_identity with valid uppercase pubkey must return ActionAccepted; got: {result:?}"
        );

        // The kernel must store the lowercase canonical form.
        assert_eq!(
            runtime.active_account_pubkey_for_test().as_deref(),
            Some(LOWER_PK),
            "active_account must be lowercase canonical hex even when input is uppercase"
        );
    }
}

#[cfg(test)]
mod resolve_no_snapshot_tests {
    //! #1436 web-feed regression guard (now on the ADR-0063 `resolve_ref` /
    //! `release_ref` seam): a resolve / release dispatch must acknowledge with
    //! `ActionAccepted` ONLY — it must NOT push a snapshot frame. Resolve/release
    //! are refcount bookkeeping and carry no new user-visible data (the resolved
    //! kind:0 arrives via the relay-pool ingest sink). On the reactive web host, a
    //! snapshot per resolve rebuilds the feed rows → remounts the avatar/name
    //! components → release + re-resolve → another snapshot — an unbounded
    //! resolve → snapshot → re-render → resolve loop that floods the
    //! single-threaded wasm worker and starves the UI so the feed never paints
    //! (feed.spec.ts toBeVisible timeout).
    use super::super::WasmRuntime;
    use crate::protocol::{ActionDispatch, WorkerEvent, WorkerRequest};

    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    /// A `resolve_ref` profile dispatch (namespace 0, shape 0 = ref, liveness 0).
    fn resolve_dispatch() -> WorkerRequest {
        WorkerRequest::Dispatch(ActionDispatch {
            action_type: "nmp.kernel.resolve_ref".to_string(),
            payload: serde_json::json!({
                "namespace": 0, "key": PK, "consumer_id": "test-consumer",
                "shape": 0, "liveness": 0,
            }),
            correlation_id: "resolve-no-snap".to_string(),
        })
    }

    /// A `release_ref` profile dispatch (namespace 0).
    fn release_dispatch() -> WorkerRequest {
        WorkerRequest::Dispatch(ActionDispatch {
            action_type: "nmp.kernel.release_ref".to_string(),
            payload: serde_json::json!({
                "namespace": 0, "key": PK, "consumer_id": "test-consumer",
            }),
            correlation_id: "release-no-snap".to_string(),
        })
    }

    fn has_update_bytes(events: &[WorkerEvent]) -> bool {
        events
            .iter()
            .any(|e| matches!(e, WorkerEvent::UpdateBytes { .. }))
    }

    fn has_accepted(events: &[WorkerEvent]) -> bool {
        events
            .iter()
            .any(|e| matches!(e, WorkerEvent::ActionAccepted { .. }))
    }

    #[test]
    fn resolve_ref_dispatch_emits_no_snapshot() {
        let mut runtime = WasmRuntime::new();
        let events = runtime
            .handle(resolve_dispatch())
            .expect("resolve_ref dispatch must succeed");
        assert!(
            has_accepted(&events),
            "resolve_ref must ACK with ActionAccepted; got {events:?}"
        );
        assert!(
            !has_update_bytes(&events),
            "resolve_ref must NOT push a snapshot frame (regression #1436); got {events:?}"
        );
    }

    #[test]
    fn release_ref_dispatch_emits_no_snapshot() {
        let mut runtime = WasmRuntime::new();
        // Resolve first so the release has something to drop (irrelevant to the
        // no-snapshot contract, but mirrors the real lifecycle).
        let _ = runtime.handle(resolve_dispatch());
        let events = runtime
            .handle(release_dispatch())
            .expect("release_ref dispatch must succeed");
        assert!(
            has_accepted(&events),
            "release_ref must ACK with ActionAccepted; got {events:?}"
        );
        assert!(
            !has_update_bytes(&events),
            "release_ref must NOT push a snapshot frame (regression #1436); got {events:?}"
        );
    }
}
