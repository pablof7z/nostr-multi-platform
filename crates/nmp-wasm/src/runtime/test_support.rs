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
        role: nmp_network::role::RelayRole,
        url: &str,
    ) -> bool {
        let outbound = self.reducer.borrow_mut().handle_relay_connected_at(
            role,
            url,
            false,
            nmp_core::time::Instant::now(),
        );
        let had_outbound = !outbound.is_empty();
        self.fan_outbound(outbound);
        if !had_outbound {
            self.request_event_drain();
        }
        had_outbound
    }

    /// Inject a relay text frame into the kernel (native test helper).
    pub fn inject_relay_text_frame_for_test(
        &mut self,
        role: nmp_network::role::RelayRole,
        url: &str,
        text: String,
    ) -> bool {
        let outbound = self.reducer.borrow_mut().handle_relay_frame_at(
            role,
            url,
            nmp_core::RelayFrame::Text(text),
            nmp_core::time::Instant::now(),
        );
        let had_outbound = !outbound.is_empty();
        self.fan_outbound(outbound);
        if !had_outbound {
            self.request_event_drain();
        }
        had_outbound
    }

    /// Pull a snapshot as raw FlatBuffers bytes (native test helper). On
    /// wasm32 every kernel mutation pushes a snapshot through the JS callback;
    /// native tests pull explicitly via this method.
    pub fn snapshot_bytes_for_test(&mut self) -> Vec<u8> {
        use crate::snapshot::build_snapshot_bytes;
        build_snapshot_bytes(&mut self.reducer.borrow_mut(), &mut self.meta.borrow_mut())
    }

    /// Fire the currently armed runtime deadline (native test helper).
    ///
    /// Production wasm uses a one-shot browser `setTimeout`; native tests fire
    /// the same scheduler state explicitly so they can prove idle runtimes do
    /// not re-arm a fixed cadence.
    pub fn fire_maintenance_deadline_for_test(
        &mut self,
    ) -> Option<(Vec<nmp_core::OutboundMessage>, bool)> {
        crate::tick::fire_deadline_for_test(
            &self.maintenance_deadline,
            &self.reducer,
            &self.post_tick_drain,
        )
        .map(|outcome| (outcome.outbound, outcome.dirty))
    }

    pub fn maintenance_deadline_armed_for_test(&self) -> bool {
        self.maintenance_deadline.borrow().armed_for_test()
    }

    pub fn maintenance_deadline_delay_for_test(&self) -> Option<u32> {
        self.maintenance_deadline.borrow().armed_delay_ms_for_test()
    }

    pub fn maintenance_deadline_requests_for_test(&self) -> u64 {
        self.maintenance_deadline.borrow().requested_for_test()
    }

    pub fn maintenance_deadline_fires_for_test(&self) -> u64 {
        self.maintenance_deadline.borrow().fired_for_test()
    }

    /// Read the active-account pubkey the kernel currently holds (native test
    /// helper). Returns the canonical lowercase hex string, or `None` if no
    /// account is active.
    pub fn active_account_pubkey_for_test(&self) -> Option<String> {
        self.reducer.borrow().active_account_pubkey()
    }

    pub fn next_runtime_deadline_delay_for_test(&self) -> Option<u32> {
        self.reducer.borrow().next_runtime_deadline_delay_ms()
    }

    /// Pin the reducer clock for deterministic native integration tests.
    pub fn set_kernel_clock_for_test(&mut self, clock: std::sync::Arc<dyn nmp_core::Clock>) {
        self.reducer.borrow_mut().set_clock_for_test(clock);
    }
}

#[cfg(test)]
mod set_identity_tests {
    //! B2 — canonicalization guard: `set_identity` with an uppercase pubkey hex
    //! must store a canonical (lowercase) active account so active-follows REQs
    //! carry the correct key in their author filters.
    use super::super::WasmRuntime;
    use crate::protocol::{SetIdentity, WorkerRequest};

    const LOWER_PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    const UPPER_PK: &str = "3BF0C63FCB93463407AF97A5E5EE64FA883D107EF9E558472C4EB9AAAEFA459D";

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
            result
                .iter()
                .any(|e| matches!(e, crate::protocol::WorkerEvent::ActionAccepted { .. })),
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
    //! `release_ref` seam): a resolve / release request must acknowledge with
    //! `ActionAccepted` ONLY — it must NOT push a snapshot frame. Resolve/release
    //! are refcount bookkeeping and carry no new user-visible data (the resolved
    //! kind:0 arrives via the relay-pool ingest sink). On the reactive web host, a
    //! snapshot per resolve rebuilds the feed rows → remounts the avatar/name
    //! components → release + re-resolve → another snapshot — an unbounded
    //! resolve → snapshot → re-render → resolve loop that floods the
    //! single-threaded wasm worker and starves the UI so the feed never paints
    //! (feed.spec.ts toBeVisible timeout).
    use super::super::WasmRuntime;
    use crate::protocol::{ReleaseRef, ResolveRef, WorkerEvent, WorkerRequest};

    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    /// A `resolve_ref` profile dispatch (namespace 0, shape 0 = ref, liveness 0).
    fn resolve_dispatch() -> WorkerRequest {
        WorkerRequest::ResolveRef(ResolveRef {
            namespace: 0,
            key: PK.to_string(),
            consumer_id: "test-consumer".to_string(),
            shape: 0,
            liveness: 0,
            hints: Vec::new(),
            event_author: None,
            correlation_id: "resolve-no-snap".to_string(),
        })
    }

    /// A `release_ref` profile dispatch (namespace 0).
    fn release_dispatch() -> WorkerRequest {
        WorkerRequest::ReleaseRef(ReleaseRef {
            namespace: 0,
            key: PK.to_string(),
            consumer_id: "test-consumer".to_string(),
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

#[cfg(test)]
mod s10_nip07_event_driven_tests {
    //! S10 (#1757) G4 — NIP-07 signer response resumes the reducer
    //! synchronously (event-driven), NOT via timer / tick / poll (D8).
    //!
    //! The wasm NIP-07 round-trip is:
    //!   1. `BeginSign` parks a sign op and emits `SignRequest`.
    //!   2. Main thread calls `window.nostr.signEvent`, gets `signed_json`.
    //!   3. Main thread posts `DeliverSignerResponse` back to the worker.
    //!   4. The worker's `handle(DeliverSignerResponse)` call returns
    //!      `[SignCompleted { … }]` IN THAT SAME CALL — no tick required.
    //!
    //! This test is LOAD-BEARING: if `deliver_signer_response` were changed
    //! to store the response and only resolve it on the next `tick()`, the
    //! returned events from `handle(DeliverSignerResponse)` would be empty
    //! (no `SignCompleted`), and the assertion `has_sign_completed` would fail.

    use super::super::WasmRuntime;
    use crate::protocol::{
        BeginSign, DeliverSignerResponse, SetIdentity, WorkerEvent, WorkerRequest,
    };

    const ACCOUNT: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    /// A minimal unsigned-event JSON that `begin_sign_roundtrip` accepts.
    fn unsigned_json() -> String {
        serde_json::json!({
            "pubkey": ACCOUNT,
            "kind": 1,
            "tags": [],
            "content": "s10 nip07 event-driven probe",
            "created_at": 1_700_000_000u64,
        })
        .to_string()
    }

    /// A flat-NIP-01 signed event JSON that `deliver_signed_response` accepts.
    fn signed_json() -> String {
        serde_json::json!({
            "id": "aa".repeat(32),
            "pubkey": ACCOUNT,
            "created_at": 1_700_000_000u64,
            "kind": 1,
            "tags": [],
            "content": "s10 nip07 event-driven probe",
            "sig": "bb".repeat(64),
        })
        .to_string()
    }

    fn has_sign_completed(events: &[WorkerEvent]) -> bool {
        events
            .iter()
            .any(|e| matches!(e, WorkerEvent::SignCompleted { .. }))
    }

    fn has_sign_request(events: &[WorkerEvent]) -> bool {
        events
            .iter()
            .any(|e| matches!(e, WorkerEvent::SignRequest { .. }))
    }

    fn sign_request_correlation_id(events: &[WorkerEvent]) -> Option<String> {
        events.iter().find_map(|e| {
            if let WorkerEvent::SignRequest { correlation_id, .. } = e {
                Some(correlation_id.clone())
            } else {
                None
            }
        })
    }

    /// S10 G4: `DeliverSignerResponse` resumes the reducer in the SAME
    /// `handle` call — no `tick_for_test`, no `sleep`, no poll loop between
    /// `BeginSign` and the `SignCompleted` event.
    ///
    /// Proof structure:
    ///   Step 1: `handle(BeginSign)` → `[SignRequest { correlation_id }]`
    ///   Step 2: `handle(DeliverSignerResponse { … })` → `[SignCompleted { … }]`
    ///
    /// The critical assertion is that step 2 returns `SignCompleted` WITHOUT
    /// any intermediate `tick_for_test()` call. If the implementation ever
    /// changed to defer completion to the next tick, step 2 would return `[]`
    /// and the `has_sign_completed` assertion would trip.
    #[test]
    fn deliver_signer_response_completes_synchronously_without_tick() {
        let mut runtime = WasmRuntime::new();

        // Seed the kernel with an active account so `begin_sign_roundtrip`
        // can validate the account-pinning contract.
        runtime
            .handle(WorkerRequest::SetIdentity(SetIdentity {
                kind: "nip07".to_string(),
                pubkey_hex: ACCOUNT.to_string(),
                correlation_id: "s10-set-id".to_string(),
            }))
            .expect("set_identity must succeed");

        // Step 1: begin the sign round-trip.
        let begin_events = runtime
            .handle(WorkerRequest::BeginSign(BeginSign {
                account_pubkey: ACCOUNT.to_string(),
                unsigned_json: unsigned_json(),
            }))
            .expect("begin_sign must succeed");

        assert!(
            has_sign_request(&begin_events),
            "BeginSign must emit a SignRequest immediately; got: {begin_events:?}"
        );

        // Extract the correlation_id the broker uses to match the response.
        let corr_id = sign_request_correlation_id(&begin_events)
            .expect("SignRequest must carry a correlation_id");

        // ── NO tick_for_test() call here — that is the assertion ──
        // The test explicitly does NOT call `runtime.tick_for_test()` between
        // BeginSign and DeliverSignerResponse. If completion required a tick
        // the next step would return an empty event list.

        // Step 2: deliver the signer response — must complete synchronously.
        let deliver_events = runtime
            .handle(WorkerRequest::DeliverSignerResponse(
                DeliverSignerResponse {
                    correlation_id: corr_id.clone(),
                    signed_json: Some(signed_json()),
                    error: None,
                },
            ))
            .expect("deliver_signer_response must not error");

        assert!(
            has_sign_completed(&deliver_events),
            "S10 G4: DeliverSignerResponse must return SignCompleted in the \
             SAME handle call (event-driven, no tick/sleep). \
             If this trips, the reducer deferred completion to a timer or poll. \
             Got events: {deliver_events:?}"
        );

        // Belt-and-suspenders: the SignCompleted must carry the matching corr_id.
        let completed_corr = deliver_events.iter().find_map(|e| {
            if let WorkerEvent::SignCompleted { correlation_id, .. } = e {
                Some(correlation_id.clone())
            } else {
                None
            }
        });
        assert_eq!(
            completed_corr.as_deref(),
            Some(corr_id.as_str()),
            "SignCompleted correlation_id must match the BeginSign round-trip id"
        );
    }
}
