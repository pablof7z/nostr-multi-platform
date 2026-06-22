use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_wasm::{
    ClientHello, DispatchBytes, RelayBootstrapEntry, RuntimeStatus, SetIdentity, StartConfig,
    WasmRuntime, WorkerEvent, WorkerRequest,
};
use serde_json::json;

/// Build a `WorkerRequest::DispatchBytes` carrying a finished `DispatchEnvelope`
/// for `action_namespace` with an opaque payload — the SAME open transport the
/// native FFI uses. This is the ONLY wasm write doorway after #1743 Cut A;
/// there is no `AppAction` enum / `"app_action"` envelope.
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
fn hello_round_trips_through_json() {
    let request = WorkerRequest::Hello(ClientHello {
        app_id: "chirp".to_string(),
        platform: "web".to_string(),
        protocol_version: 1,
    });

    let json = serde_json::to_string(&request).unwrap();
    let decoded: WorkerRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded, request);
}

// The `StartConfig` relay-policy deserialization contract (#1125) lives in
// `tests/start_config.rs` to keep this file under the 500-LOC hard cap.

#[test]
fn start_runs_browser_wasm_facade_with_shared_relay_defaults() {
    let mut runtime = WasmRuntime::new();

    let events = runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec!["wss://relay.example".to_string()],
            relay_bootstrap: vec![RelayBootstrapEntry {
                url: "wss://relay.example".to_string(),
                role: "both,indexer".to_string(),
            }],
            database_name: "chirp-dev".to_string(),
            correlation_id: "start-1".to_string(),
        }))
        .unwrap();

    assert_eq!(
        events[0],
        WorkerEvent::RuntimeStatus {
            status: RuntimeStatus::Running,
            correlation_id: Some("start-1".to_string()),
        }
    );
    assert!(matches!(events[1], WorkerEvent::UpdateBytes { .. }));
}

#[test]
fn update_bytes_event_round_trips_without_json_snapshot_envelope() {
    let event = WorkerEvent::UpdateBytes {
        bytes: vec![0x4e, 0x4d, 0x50, 0x46],
    };

    let encoded = serde_json::to_string(&event).unwrap();
    assert!(encoded.contains(r#""type":"update_bytes""#));
    assert!(!encoded.contains(r#""envelope""#));

    let decoded: WorkerEvent = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, event);
}

#[test]
fn invalid_protocol_is_rejected_before_start() {
    let mut runtime = WasmRuntime::new();

    let events = runtime
        .handle(WorkerRequest::Hello(ClientHello {
            app_id: "chirp".to_string(),
            platform: "web".to_string(),
            protocol_version: 2,
        }))
        .unwrap();

    match &events[0] {
        WorkerEvent::Error { code, .. } => assert_eq!(code, "protocol_mismatch"),
        other => panic!("expected protocol error, got {other:?}"),
    }
}

/// ADR-0064 / #1743: there is NO wasm-only write vocabulary. The deleted
/// `"app_action"` envelope must fail serde decode — a host that still sends it
/// gets a loud rejection, not a silent accept through a back-compat alias.
#[test]
fn deleted_app_action_envelope_does_not_deserialize() {
    let result: Result<WorkerRequest, _> = serde_json::from_value(json!({
        "type": "app_action",
        "correlation_id": "react-1",
        "action": { "action": "react", "target_event_id": "event-id" }
    }));
    assert!(
        result.is_err(),
        "the `app_action` envelope was deleted in #1743 Cut A; it must NOT \
         round-trip through serde, got: {result:?}"
    );
}

/// A wasm write routes through the generic typed `DispatchEnvelope` over the one
/// `DispatchBytes` doorway — identical in shape to the native FFI seam. The
/// envelope's `correlation_id` + `action_namespace` survive the decode, and the
/// runtime routes by namespace (publishing itself stays honestly-disabled in the
/// web preview, #1008 — but it crossed via the TYPED path, not `AppAction`).
#[test]
fn typed_write_routes_through_dispatch_envelope_not_app_action() {
    let mut runtime = WasmRuntime::new();
    let events = runtime
        .handle(dispatch_bytes_request("follow-1", "nmp.follow", b"opaque-payload"))
        .unwrap();

    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            // The namespace from the DECODED envelope is echoed back — proof the
            // typed envelope crossed and routed by `action_namespace`.
            assert_eq!(failure.capability, "nmp.follow");
            assert_eq!(failure.correlation_id, "follow-1");
            // No active account yet → "sign in to publish".
            assert!(
                failure.reason.starts_with("signer_not_installed"),
                "expected signer_not_installed prefix, got: {}",
                failure.reason
            );
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}

/// A malformed write buffer (not a `DispatchEnvelope` root) fails CLOSED with a
/// data-shaped `Error` — never a panic, never a silent accept (D6).
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

// V-01 Stage 2: `WasmRuntime` no longer keeps a local `Vec<LocalNote>` and no
// longer fabricates a snapshot that "contains" the published note. The pure
// `KernelReducer` runs in WASM, but the actor + relay_worker (and therefore
// every signed-event publish path) are `#[cfg(feature = "native")]` and
// unreachable. The honest contract for app-level intents in browser WASM
// today is `CapabilityFailure(browser_actor_driver_missing)`; Stage 3 will
// wire `web_sys::WebSocket` so these complete.

#[test]
fn start_emits_flatbuffer_snapshot_update_from_real_kernel() {
    let mut runtime = WasmRuntime::new();
    let events = runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec!["wss://relay.example".to_string()],
            relay_bootstrap: vec![RelayBootstrapEntry {
                url: "wss://relay.example".to_string(),
                role: "both,indexer".to_string(),
            }],
            database_name: "chirp-dev".to_string(),
            correlation_id: "start-1".to_string(),
        }))
        .unwrap();

    assert_eq!(events.len(), 2, "Start must emit RuntimeStatus + Update");
    let WorkerEvent::UpdateBytes { bytes } = &events[1] else {
        panic!("expected update bytes, got {:?}", events[1]);
    };

    assert!(!bytes.is_empty());
    assert!(
        bytes.windows(4).any(|window| window == b"NMPU"),
        "FlatBuffers update must carry the NMPU file identifier"
    );
    assert!(
        !String::from_utf8_lossy(bytes).contains(r#""t":"snapshot""#),
        "snapshot update transport must not be the legacy JSON envelope"
    );
}

#[test]
fn typed_write_without_active_account_returns_signer_not_installed() {
    let mut runtime = WasmRuntime::new();

    let events = runtime
        .handle(dispatch_bytes_request("pub-1", "nmp.publish", b"opaque"))
        .unwrap();

    // With no active account seeded, app-level writes fail with the honest
    // `signer_not_installed` token ("sign in to publish").
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
fn typed_write_after_set_identity_returns_publish_disabled_token() {
    let mut runtime = WasmRuntime::new();

    // Seed the active identity (NO persistent signer is installed — ADR-0064
    // §5; `SetIdentity` only validates/canonicalizes the pubkey and sets the
    // kernel active account).
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

    // Now the same typed write surfaces the *second* honest state: an account
    // is active but publishing is disabled in the web preview (#1202/#1008).
    // Hosts distinguish "you need to sign in" (`signer_not_installed`) from
    // "publishing is disabled" by pattern-matching the one canonical prefix.
    let events = runtime
        .handle(dispatch_bytes_request("pub-1", "nmp.publish", b"opaque"))
        .unwrap();
    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.publish");
            assert!(
                failure
                    .reason
                    .starts_with("publish_not_supported_in_web_preview"),
                "expected the canonical publish-disabled token, got: {}",
                failure.reason
            );
            assert!(
                !failure.reason.starts_with("publish_path_not_wired"),
                "legacy publish_path_not_wired token must be gone, got: {}",
                failure.reason
            );
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}

#[test]
fn set_identity_with_unknown_kind_returns_unsupported_signer_kind() {
    let mut runtime = WasmRuntime::new();
    let events = runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "magic".to_string(),
            pubkey_hex: String::new(),
            correlation_id: "set-1".to_string(),
        }))
        .unwrap();
    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.set_identity");
            assert_eq!(failure.correlation_id, "set-1");
            assert!(
                failure.reason.starts_with("unsupported_signer_kind"),
                "expected unsupported_signer_kind prefix, got: {}",
                failure.reason
            );
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}

#[test]
fn set_identity_with_garbage_hex_returns_invalid_signer_pubkey() {
    let mut runtime = WasmRuntime::new();
    let events = runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "nip07".to_string(),
            pubkey_hex: "not-hex".to_string(),
            correlation_id: "set-1".to_string(),
        }))
        .unwrap();
    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.set_identity");
            assert!(
                failure.reason.starts_with("invalid_signer_pubkey"),
                "expected invalid_signer_pubkey prefix, got: {}",
                failure.reason
            );
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}

#[test]
fn set_identity_serde_round_trip_through_json() {
    // The wasm-bindgen `handle_json` entry point deserialises every
    // WorkerRequest from JSON, so the SetIdentity variant must round-trip
    // through serde with the snake-cased tag the JS host sends.
    let request: WorkerRequest = serde_json::from_value(json!({
        "type": "set_identity",
        "kind": "nip07",
        "pubkey_hex": "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
        "correlation_id": "set-1",
    }))
    .unwrap();

    match request {
        WorkerRequest::SetIdentity(set) => {
            assert_eq!(set.kind, "nip07");
            assert_eq!(set.correlation_id, "set-1");
        }
        other => panic!("expected SetIdentity, got {other:?}"),
    }
}

#[test]
fn kernel_namespaced_dispatch_routes_through_real_reducer() {
    use nmp_wasm::ActionDispatch;
    let mut runtime = WasmRuntime::new();

    // `nmp.kernel.start` is one of the action_types the runtime routes
    // directly to `KernelReducer::reduce(KernelAction::Start)`. Proves the
    // generic Dispatch path is wired to the real kernel — not a hardcoded
    // string match against a fake snapshot.
    let events = runtime
        .handle(WorkerRequest::Dispatch(ActionDispatch {
            action_type: "nmp.kernel.start".to_string(),
            payload: serde_json::json!({}),
            correlation_id: "k-start-1".to_string(),
        }))
        .unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0],
        WorkerEvent::ActionAccepted {
            action_type: "nmp.kernel.start".to_string(),
            correlation_id: "k-start-1".to_string(),
        }
    );
    let WorkerEvent::UpdateBytes { bytes } = &events[1] else {
        panic!("expected update bytes, got {:?}", events[1]);
    };
    assert!(bytes.windows(4).any(|window| window == b"NMPU"));
}

#[test]
fn app_namespaced_dispatch_without_signer_returns_signer_not_installed() {
    use nmp_wasm::ActionDispatch;
    let mut runtime = WasmRuntime::new();

    // `nmp.publish` is an *app* action — it produces a signed event. With
    // no signer slot filled, the runtime returns the Stage 3b
    // signer-precise error rather than fabricating a snapshot the way the
    // pre-Stage-2 stub did.
    let events = runtime
        .handle(WorkerRequest::Dispatch(ActionDispatch {
            action_type: "nmp.publish".to_string(),
            payload: serde_json::json!({"PublishRaw": {"kind": 1, "tags": [], "content": "hi", "target": "Auto"}}),
            correlation_id: "pub-2".to_string(),
        }))
        .unwrap();

    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.publish");
            assert!(failure.reason.starts_with("signer_not_installed"));
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}

// V-51 phase 2 — routing-trace JSON snapshot accessor. Mirrors the iOS
// `nmp_app_recent_routing_decisions` FFI symbol on the wasm surface so the
// web Chirp shell (phase 3) can render the same routing inspector.
#[test]
fn recent_routing_decisions_returns_schema_versioned_json() {
    let runtime = WasmRuntime::new();
    let json = runtime.recent_routing_decisions();
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(value["schema_version"], 1);
    // Fresh kernel: both rings are empty. The accessor must still emit
    // well-formed array shells (D6 — never null, never undefined).
    assert!(value["publishes"].is_array());
    assert!(value["subscriptions"].is_array());
    assert_eq!(value["publishes"].as_array().unwrap().len(), 0);
    assert_eq!(value["subscriptions"].as_array().unwrap().len(), 0);
}

// PR-1 acceptance test (kernel-authored snapshot) lives in snapshot_protocol.rs
// to keep this file under the 500-LOC hard cap (AGENTS.md).
