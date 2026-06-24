use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_wasm::{DispatchBytes, SetIdentity, WasmRuntime, WorkerEvent, WorkerRequest};

/// Build a `WorkerRequest::DispatchBytes` carrying a finished `DispatchEnvelope`
/// for `action_namespace` with an opaque payload. This is the only wasm write
/// doorway after #1743 Cut A.
fn dispatch_bytes_request(
    correlation_id: &str,
    action_namespace: &str,
    payload: &[u8],
) -> WorkerRequest {
    let bytes = encode_dispatch_envelope(
        correlation_id,
        action_namespace,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        payload,
    );
    WorkerRequest::DispatchBytes(DispatchBytes { bytes })
}

#[test]
fn typed_write_routes_through_dispatch_envelope_not_app_action() {
    let mut runtime = WasmRuntime::new();
    let events = runtime
        .handle(dispatch_bytes_request(
            "follow-1",
            "nmp.follow",
            b"opaque-payload",
        ))
        .unwrap();

    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.follow");
            assert_eq!(failure.correlation_id, "follow-1");
            assert!(
                failure.reason.starts_with("signer_not_installed"),
                "expected signer_not_installed prefix, got: {}",
                failure.reason
            );
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}

#[test]
fn dispatch_bytes_rejects_non_envelope_buffer() {
    let mut runtime = WasmRuntime::new();
    let events = runtime
        .handle(WorkerRequest::DispatchBytes(DispatchBytes {
            bytes: b"not a flatbuffer".to_vec(),
        }))
        .unwrap();
    match &events[0] {
        WorkerEvent::Error { code, .. } => {
            assert_eq!(code, "dispatch_envelope_rejected");
        }
        other => panic!("expected dispatch_envelope_rejected Error, got {other:?}"),
    }
}

#[test]
fn typed_write_without_active_account_returns_signer_not_installed() {
    let mut runtime = WasmRuntime::new();

    let events = runtime
        .handle(dispatch_bytes_request("pub-1", "nmp.publish", b"opaque"))
        .unwrap();

    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.publish");
            assert_eq!(failure.correlation_id, "pub-1");
            assert!(
                failure.reason.starts_with("signer_not_installed"),
                "expected signer_not_installed prefix, got: {}",
                failure.reason
            );
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}

#[test]
fn typed_write_after_set_identity_fails_at_decode_after_1008() {
    let mut runtime = WasmRuntime::new();

    let set_events = runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "nip07".to_string(),
            pubkey_hex: "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
                .to_string(),
            correlation_id: "set-1".to_string(),
        }))
        .unwrap();
    match &set_events[0] {
        WorkerEvent::ActionAccepted {
            action_type,
            correlation_id,
        } => {
            assert_eq!(action_type, "nmp.set_identity");
            assert_eq!(correlation_id, "set-1");
        }
        other => panic!("expected ActionAccepted, got {other:?}"),
    }

    let events = runtime
        .handle(dispatch_bytes_request("pub-1", "nmp.publish", b"opaque"))
        .unwrap();
    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.publish");
            assert!(
                !failure
                    .reason
                    .starts_with("publish_not_supported_in_web_preview"),
                "after #1008 the old publish-disabled token must be gone, got: {}",
                failure.reason
            );
            assert!(
                !failure.reason.starts_with("publish_path_not_wired"),
                "legacy publish_path_not_wired token must be gone, got: {}",
                failure.reason
            );
            assert!(
                failure.reason.contains("malformed")
                    || failure.reason.contains("file identifier")
                    || failure.reason.contains("decode")
                    || failure.reason.contains("invalid"),
                "expected a decode-rejection reason after #1008; got: {}",
                failure.reason
            );
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}
