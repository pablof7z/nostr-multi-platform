//! ADR-0064 / S2 (#1750) + S3 (#1751) — the wasm binary write doorway
//! (`dispatch_bytes`).
//!
//! S2 proves the raw-byte inbound channel: a transferable `Uint8Array` of a
//! finished `DispatchEnvelope` crosses, decodes through the ONE core decode
//! path, and routes by `action_namespace`. Fail-closed cases surface a
//! data-shaped `WorkerEvent::Error` — never a panic, never a silent accept.
//!
//! S3 (#1751 / #1008) proves the OPAQUE per-crate payload now routes into the
//! runtime's typed [`nmp_wasm::WasmRuntime`] action registry: a registered
//! non-publish namespace's typed FlatBuffers payload DECODES (via the module's
//! `decode_payload`) and reaches the module's `start()` validator, instead of
//! returning the generic envelope-level write-path `CapabilityFailure` the S2
//! arm produced for every namespace.

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_wasm::{CapabilityFailure, SetIdentity, WasmRuntime, WorkerEvent, WorkerRequest};

fn envelope(namespace: &str, correlation: &str, version: u32, payload: &[u8]) -> Vec<u8> {
    encode_dispatch_envelope(correlation, namespace, version, payload)
}

/// Seed an active account so a dispatch passes the fail-closed
/// `signer_not_installed` gate and reaches the typed registry.
fn seed_account(runtime: &mut WasmRuntime) {
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let events = runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "nip07".to_string(),
            pubkey_hex: PK.to_string(),
            correlation_id: "seed".to_string(),
        }))
        .expect("set_identity must succeed");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, WorkerEvent::ActionAccepted { .. })),
        "seed_account: SetIdentity must ACK; got {events:?}"
    );
}

#[test]
fn binary_doorway_decodes_and_routes_by_namespace() {
    let mut runtime = WasmRuntime::new();
    // Opaque payload — the binary lane carries it verbatim, never interpreted.
    let bytes = envelope(
        "nmp.publish",
        "corr-1",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        b"\x01\x02\x03",
    );

    let events = runtime.dispatch_bytes(&bytes);

    // No signer installed → the honest write-path reason, keyed to the decoded
    // namespace + correlation_id. Proves the envelope crossed and decoded.
    assert!(matches!(
        &events[..],
        [WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability,
            correlation_id,
            ..
        })] if capability == "nmp.publish" && correlation_id == "corr-1"
    ));
}

#[test]
fn binary_doorway_rejects_schema_version_mismatch_fail_closed() {
    let mut runtime = WasmRuntime::new();
    let bad = envelope(
        "nmp.publish",
        "corr-2",
        DISPATCH_ENVELOPE_SCHEMA_VERSION + 1,
        b"p",
    );

    let events = runtime.dispatch_bytes(&bad);

    // Fail closed: a data-shaped error, NOT a routed dispatch.
    assert!(matches!(
        &events[..],
        [WorkerEvent::Error { code, .. }] if code == "dispatch_envelope_rejected"
    ));
}

#[test]
fn binary_doorway_rejects_wrong_file_identifier_fail_closed() {
    let mut runtime = WasmRuntime::new();
    let mut bytes = envelope("nmp.publish", "c", DISPATCH_ENVELOPE_SCHEMA_VERSION, b"p");
    bytes[4..8].copy_from_slice(b"NMPU"); // read-direction magic
    let events = runtime.dispatch_bytes(&bytes);
    assert!(matches!(
        &events[..],
        [WorkerEvent::Error { code, .. }] if code == "dispatch_envelope_rejected"
    ));
}

#[test]
fn binary_doorway_rejects_garbage_fail_closed() {
    let mut runtime = WasmRuntime::new();
    let events = runtime.dispatch_bytes(b"not a flatbuffer");
    assert!(matches!(
        &events[..],
        [WorkerEvent::Error { code, .. }] if code == "dispatch_envelope_rejected"
    ));
}

// ---------------------------------------------------------------------------
// ADR-0064 / S3 (#1751 / #1008) — the typed-payload DECODE arm.
// ---------------------------------------------------------------------------

mod typed_decode {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use nmp_core::actor::ActorCommand;
    use nmp_core::substrate::{
        ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
    };
    use serde::{Deserialize, Serialize};

    const PROBE_NAMESPACE: &str = "nmp.test.typed_probe";
    /// The opaque payload the test stamps into the envelope: a marker the
    /// module's `decode_payload` recognises (proving the per-crate typed-decode
    /// site, not the envelope decode, ran).
    const PROBE_PAYLOAD: &[u8] = b"PROBE-OK";

    #[derive(Clone, Serialize, Deserialize)]
    struct ProbeAction {
        marker: String,
    }

    /// The `ActionPayload` codec for the probe action — the typed-decode site
    /// the registry adapter calls. A non-marker buffer fails closed; this proves
    /// `start_bytes` reaches the per-crate `decode_payload`, not just the
    /// envelope.
    impl ActionPayload for ProbeAction {
        const SCHEMA_ID: &'static str = PROBE_NAMESPACE;
        const SCHEMA_VERSION: u32 = 1;

        fn encode(&self) -> Vec<u8> {
            self.marker.as_bytes().to_vec()
        }
        fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
            if bytes == PROBE_PAYLOAD {
                Ok(ProbeAction {
                    marker: "PROBE-OK".to_string(),
                })
            } else {
                Err(ActionPayloadDecodeError::Malformed {
                    reason: "probe payload marker mismatch".to_string(),
                })
            }
        }
    }

    /// A typed-payload-capable module whose `start()` flips a shared flag, so a
    /// test can prove the dispatched typed payload DECODED and REACHED `start()`
    /// (rather than returning the generic envelope-level `CapabilityFailure`).
    struct ProbeModule {
        started: Arc<AtomicBool>,
    }

    impl ActionModule for ProbeModule {
        const NAMESPACE: &'static str = PROBE_NAMESPACE;
        type Action = ProbeAction;

        fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
            Some(<ProbeAction as ActionPayload>::decode(bytes))
        }

        fn start(
            &self,
            _ctx: &mut ActionContext,
            action: Self::Action,
        ) -> Result<(), ActionRejection> {
            // Proves the typed decode produced the action AND start() ran.
            assert_eq!(action.marker, "PROBE-OK");
            self.started.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn execute(
            &self,
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(ActorCommand),
        ) -> Result<(), String> {
            Ok(())
        }
    }

    /// Load-bearing S3 test (#1008): a typed payload dispatched through the wasm
    /// `dispatch_bytes` doorway for a REGISTERED non-publish namespace DECODES
    /// per-crate, REACHES the module's `start()`, and EXECUTES — returning
    /// `ActionAccepted + UpdateBytes` for a module whose `execute()` emits no
    /// `ActorCommand`s (side-effect-only module). This proves the full
    /// start_bytes + execute_bytes path runs, NOT the generic envelope-level
    /// `CapabilityFailure`.
    #[test]
    fn typed_payload_decodes_and_reaches_start_not_capability_failure() {
        let started = Arc::new(AtomicBool::new(false));
        let mut runtime = WasmRuntime::new();
        // The composition root registers per-NIP modules; here the test plays
        // that role via the same public `register_action` seam.
        let _ = runtime.register_action(ProbeModule {
            started: Arc::clone(&started),
        });
        // An active account is required (fail-closed: no signer → no write).
        seed_account(&mut runtime);
        let _ = runtime
            .fire_maintenance_deadline_for_test()
            .expect("seed_account must arm a post-event drain");
        let _ = runtime.snapshot_bytes_for_test();
        assert!(
            !runtime.maintenance_deadline_armed_for_test(),
            "typed dispatch assertion must start from an unarmed scheduler"
        );

        let bytes = envelope(
            PROBE_NAMESPACE,
            "corr-typed",
            DISPATCH_ENVELOPE_SCHEMA_VERSION,
            PROBE_PAYLOAD,
        );
        let events = runtime.dispatch_bytes(&bytes);

        // THE load-bearing assertion: the typed payload reached start().
        assert!(
            started.load(Ordering::SeqCst),
            "typed payload must DECODE + reach the module's start(); got {events:?}"
        );
        // ProbeModule::execute() emits no ActorCommands → the empty-commands
        // arm returns ActionAccepted + UpdateBytes (not a CapabilityFailure).
        // This is the #1008 outcome: the typed dispatch path runs to completion
        // instead of short-circuiting with a "not yet wired" token.
        match &events[..] {
            [WorkerEvent::ActionAccepted {
                action_type,
                correlation_id,
            }, WorkerEvent::UpdateBytes { .. }] => {
                assert_eq!(action_type, PROBE_NAMESPACE);
                assert_eq!(correlation_id, "corr-typed");
            }
            other => panic!(
                "expected [ActionAccepted, UpdateBytes] for side-effect-only module; got {other:?}"
            ),
        }
        assert_eq!(
            runtime.maintenance_deadline_delay_for_test(),
            Some(1_000),
            "successful typed command execution must arm one post-event drain"
        );
    }

    /// Fail-closed: a malformed typed payload for a registered namespace is
    /// REJECTED at `start_bytes` (the per-crate `decode_payload` trips) and
    /// never reaches `start()` — the raw decode reason is surfaced.
    #[test]
    fn malformed_typed_payload_fails_closed_at_decode() {
        let started = Arc::new(AtomicBool::new(false));
        let mut runtime = WasmRuntime::new();
        let _ = runtime.register_action(ProbeModule {
            started: Arc::clone(&started),
        });
        seed_account(&mut runtime);

        let bytes = envelope(
            PROBE_NAMESPACE,
            "corr-bad",
            DISPATCH_ENVELOPE_SCHEMA_VERSION,
            b"WRONG-MARKER",
        );
        let events = runtime.dispatch_bytes(&bytes);

        assert!(
            !started.load(Ordering::SeqCst),
            "a malformed payload must NOT reach start()"
        );
        match &events[..] {
            [WorkerEvent::CapabilityFailure(CapabilityFailure { reason, .. })] => {
                assert!(
                    reason.contains("marker mismatch") || reason.contains("Malformed"),
                    "decode failure must surface the raw reason; got: {reason}"
                );
            }
            other => panic!("expected a decode-rejection CapabilityFailure; got {other:?}"),
        }
    }

    /// Fail-closed: an UNREGISTERED namespace (even with an active account)
    /// still rejects through `start_bytes` as unknown — never a silent accept.
    #[test]
    fn unknown_namespace_fails_closed() {
        let mut runtime = WasmRuntime::new();
        seed_account(&mut runtime);
        let bytes = envelope(
            "nmp.test.never_registered",
            "corr-unk",
            DISPATCH_ENVELOPE_SCHEMA_VERSION,
            PROBE_PAYLOAD,
        );
        let events = runtime.dispatch_bytes(&bytes);
        match &events[..] {
            [WorkerEvent::CapabilityFailure(CapabilityFailure { reason, .. })] => {
                assert!(
                    reason.contains("unknown action namespace"),
                    "an unregistered namespace must reject as unknown; got: {reason}"
                );
            }
            other => panic!("expected an unknown-namespace CapabilityFailure; got {other:?}"),
        }
    }

    /// After #1008: `nmp.publish` now routes through the typed registry and
    /// `WasmOutboxResolver` is wired in. A non-FlatBuffers payload (like the
    /// `PROBE_PAYLOAD` marker bytes) reaches `PublishModule::decode_payload`
    /// which rejects it as malformed — NOT the old "outbox resolver not wired"
    /// token. This proves the publish short-circuit skip is GONE.
    #[test]
    fn publish_malformed_payload_fails_closed_at_decode_after_1008() {
        let mut runtime = WasmRuntime::new();
        seed_account(&mut runtime);
        let bytes = envelope(
            "nmp.publish",
            "corr-pub",
            DISPATCH_ENVELOPE_SCHEMA_VERSION,
            PROBE_PAYLOAD, // not a valid FlatBuffers PublishAction payload
        );
        let events = runtime.dispatch_bytes(&bytes);
        match &events[..] {
            [WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability, reason, ..
            })] => {
                assert_eq!(capability, "nmp.publish");
                // Decode rejection — not the old publish-disabled token.
                assert!(
                    !reason.starts_with("publish_not_supported_in_web_preview"),
                    "after #1008 the old disable token must be gone; got: {reason}"
                );
                assert!(
                    reason.contains("malformed")
                        || reason.contains("file identifier")
                        || reason.contains("decode")
                        || reason.contains("invalid"),
                    "malformed payload must surface a decode rejection reason; got: {reason}"
                );
            }
            other => panic!("expected publish decode CapabilityFailure; got {other:?}"),
        }
    }
}
