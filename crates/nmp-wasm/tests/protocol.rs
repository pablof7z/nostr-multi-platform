use nmp_wasm::{
    ClientHello, RelayBootstrapEntry, ResolveRef, RuntimeStatus, SetIdentity, StartConfig,
    RawWasmAbiAdapter, WorkerEvent, WorkerRequest,
};
use serde_json::json;
use std::sync::Arc;

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
    let mut runtime = RawWasmAbiAdapter::new();

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
fn start_consumes_injected_event_store_before_snapshot() {
    let store: Arc<dyn nmp_store::EventStore> = Arc::new(nmp_store::MemEventStore::new());
    let mut runtime = RawWasmAbiAdapter::new();
    runtime
        .set_injected_store(Arc::clone(&store))
        .expect("pre-start event-store injection must succeed");

    let events = runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec!["wss://relay.example".to_string()],
            relay_bootstrap: vec![RelayBootstrapEntry {
                url: "wss://relay.example".to_string(),
                role: "both,indexer".to_string(),
            }],
            database_name: "chirp-dev".to_string(),
            correlation_id: "start-injected-store".to_string(),
        }))
        .unwrap();

    assert!(matches!(events[1], WorkerEvent::UpdateBytes { .. }));
    let reducer_store = runtime.reducer_handle().borrow().event_store_handle();
    assert!(
        Arc::ptr_eq(&store, &reducer_store),
        "Start must rebuild the reducer around the injected EventStore before emitting snapshots"
    );
}

#[test]
fn event_store_injection_after_start_is_rejected() {
    let mut runtime = RawWasmAbiAdapter::new();
    runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec!["wss://relay.example".to_string()],
            relay_bootstrap: vec![RelayBootstrapEntry {
                url: "wss://relay.example".to_string(),
                role: "both,indexer".to_string(),
            }],
            database_name: "chirp-dev".to_string(),
            correlation_id: "start-before-inject".to_string(),
        }))
        .unwrap();

    let store: Arc<dyn nmp_store::EventStore> = Arc::new(nmp_store::MemEventStore::new());
    let err = runtime
        .set_injected_store(store)
        .expect_err("post-start event-store injection must fail closed");
    assert!(
        err.to_string().contains("before Start"),
        "post-start rejection should explain the boot-time contract, got: {err}"
    );
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
    let mut runtime = RawWasmAbiAdapter::new();

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

/// #1923: the retired public JSON action-dispatch envelope must fail serde
/// decode. App writes use `dispatch_bytes`; reference control uses structured
/// `resolve_ref` / `release_ref` messages.
#[test]
fn retired_json_dispatch_envelope_does_not_deserialize() {
    let result: Result<WorkerRequest, _> = serde_json::from_value(json!({
        "type": "dispatch",
        "action_type": "nmp.publish",
        "payload": { "content": "hello" },
        "correlation_id": "dispatch-1",
    }));
    assert!(
        result.is_err(),
        "the public JSON dispatch envelope is retired; it must not deserialize, got: {result:?}"
    );
}

// V-01 Stage 2: `RawWasmAbiAdapter` no longer keeps a local `Vec<LocalNote>` and no
// longer fabricates a snapshot that "contains" the published note. The pure
// `KernelReducer` runs in WASM, but the actor + relay_worker (and therefore
// every signed-event publish path) are `#[cfg(feature = "native")]` and
// unreachable. The honest contract for app-level intents in browser WASM
// today is `CapabilityFailure(browser_actor_driver_missing)`; Stage 3 will
// wire `web_sys::WebSocket` so these complete.

#[test]
fn start_emits_flatbuffer_snapshot_update_from_real_kernel() {
    let mut runtime = RawWasmAbiAdapter::new();
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
fn set_identity_with_unknown_kind_returns_unsupported_signer_kind() {
    let mut runtime = RawWasmAbiAdapter::new();
    let events = runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "magic".to_string(),
            pubkey_hex: String::new(),
            correlation_id: "set-1".to_string(),
            identity_relays: Vec::new(),
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
    let mut runtime = RawWasmAbiAdapter::new();
    let events = runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "nip07".to_string(),
            pubkey_hex: "not-hex".to_string(),
            correlation_id: "set-1".to_string(),
            identity_relays: Vec::new(),
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
fn resolve_ref_round_trips_through_json() {
    let request: WorkerRequest = serde_json::from_value(json!({
        "type": "resolve_ref",
        "namespace": 0,
        "key": "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
        "consumer_id": "chirp-web-author-1",
        "shape": 0,
        "liveness": 0,
        "correlation_id": "resolve-1",
    }))
    .unwrap();

    match request {
        WorkerRequest::ResolveRef(resolve) => {
            assert_eq!(resolve.namespace, 0);
            assert_eq!(resolve.consumer_id, "chirp-web-author-1");
            assert!(resolve.hints.is_empty());
            assert!(resolve.event_author.is_none());
            assert_eq!(resolve.correlation_id, "resolve-1");
        }
        other => panic!("expected ResolveRef, got {other:?}"),
    }
}

#[test]
fn resolve_ref_preserves_relay_hints_through_json() {
    let event_author = "ab".repeat(32);
    let request: WorkerRequest = serde_json::from_value(json!({
        "type": "resolve_ref",
        "namespace": 1,
        "key": "event-id",
        "consumer_id": "embed-1",
        "shape": 0,
        "liveness": 0,
        "hints": ["wss://relay.a.example", "wss://relay.b.example"],
        "event_author": event_author,
        "correlation_id": "resolve-event-1",
    }))
    .unwrap();

    match request {
        WorkerRequest::ResolveRef(resolve) => {
            assert_eq!(
                resolve.hints,
                vec![
                    "wss://relay.a.example".to_string(),
                    "wss://relay.b.example".to_string(),
                ]
            );
            assert_eq!(resolve.event_author, Some("ab".repeat(32)));
            assert_eq!(resolve.correlation_id, "resolve-event-1");
        }
        other => panic!("expected ResolveRef, got {other:?}"),
    }
}

#[test]
fn release_ref_round_trips_through_json() {
    let request: WorkerRequest = serde_json::from_value(json!({
        "type": "release_ref",
        "namespace": 1,
        "key": "event-id",
        "consumer_id": "chirp-web-embed-1",
        "correlation_id": "release-1",
    }))
    .unwrap();

    match request {
        WorkerRequest::ReleaseRef(release) => {
            assert_eq!(release.namespace, 1);
            assert_eq!(release.consumer_id, "chirp-web-embed-1");
            assert_eq!(release.correlation_id, "release-1");
        }
        other => panic!("expected ReleaseRef, got {other:?}"),
    }
}

#[test]
fn resolve_ref_routes_through_structured_control_message() {
    let mut runtime = RawWasmAbiAdapter::new();

    let events = runtime
        .handle(WorkerRequest::ResolveRef(ResolveRef {
            namespace: 0,
            key: "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d".to_string(),
            consumer_id: "profile-card".to_string(),
            shape: 0,
            liveness: 0,
            hints: Vec::new(),
            event_author: None,
            correlation_id: "resolve-1".to_string(),
        }))
        .unwrap();

    assert_eq!(
        events[0],
        WorkerEvent::ActionAccepted {
            action_type: "nmp.kernel.resolve_ref".to_string(),
            correlation_id: "resolve-1".to_string(),
        }
    );
}

#[test]
fn invalid_resolve_ref_returns_data_failure() {
    let mut runtime = RawWasmAbiAdapter::new();

    let events = runtime
        .handle(WorkerRequest::ResolveRef(ResolveRef {
            namespace: 99,
            key: "bad".to_string(),
            consumer_id: "profile-card".to_string(),
            shape: 0,
            liveness: 0,
            hints: Vec::new(),
            event_author: None,
            correlation_id: "resolve-1".to_string(),
        }))
        .unwrap();

    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.kernel.resolve_ref");
            assert_eq!(failure.correlation_id, "resolve-1");
            assert!(failure.reason.starts_with("invalid_ref_request"));
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}

// V-51 phase 2 — routing-trace JSON snapshot accessor. Mirrors the iOS
// `nmp_app_recent_routing_decisions` FFI symbol on the wasm surface so the
// web Chirp shell (phase 3) can render the same routing inspector.
#[test]
fn recent_routing_decisions_returns_schema_versioned_json() {
    let runtime = RawWasmAbiAdapter::new();
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
