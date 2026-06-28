use nmp_wasm::{
    BeginSign, ClientHello, DispatchBytes, RelayBootstrapEntry, RuntimeStatus, SetIdentity,
    StartConfig, WorkerEvent, WorkerRequest,
};
use serde_json::json;

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

#[test]
fn start_config_requires_explicit_relay_policy() {
    let missing_relays: Result<WorkerRequest, _> = serde_json::from_value(json!({
        "type": "start",
        "app_id": "chirp",
        "database_name": "chirp-dev",
        "correlation_id": "start-1"
    }));
    assert!(
        missing_relays.is_err(),
        "StartConfig must not synthesize framework relay defaults"
    );

    let request = WorkerRequest::Start(StartConfig {
        app_id: "chirp".to_string(),
        relays: vec!["wss://relay.example".to_string()],
        relay_bootstrap: vec![RelayBootstrapEntry {
            url: "wss://relay.example".to_string(),
            role: "both,indexer".to_string(),
        }],
        database_name: "chirp-dev".to_string(),
        correlation_id: "start-1".to_string(),
    });
    let encoded = serde_json::to_string(&request).unwrap();
    let decoded: WorkerRequest = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, request);
}

#[test]
fn dispatch_bytes_preserves_raw_byte_arrays() {
    let request = WorkerRequest::DispatchBytes(DispatchBytes {
        bytes: vec![0x4e, 0x4d, 0x50, 0x44],
    });
    let encoded = serde_json::to_string(&request).unwrap();
    assert!(encoded.contains(r#""type":"dispatch_bytes""#));
    let decoded: WorkerRequest = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, request);
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
fn signer_round_trip_protocol_is_account_pinned() {
    let request = WorkerRequest::BeginSign(BeginSign {
        account_pubkey: "a".repeat(64),
        unsigned_json: r#"{"kind":1}"#.to_string(),
    });
    let encoded = serde_json::to_string(&request).unwrap();
    let decoded: WorkerRequest = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, request);

    let identity = WorkerRequest::SetIdentity(SetIdentity {
        kind: "nip07".to_string(),
        pubkey_hex: "b".repeat(64),
        correlation_id: "identity-1".to_string(),
        identity_relays: Vec::new(),
    });
    let encoded_identity = serde_json::to_string(&identity).unwrap();
    assert_eq!(
        serde_json::from_str::<WorkerRequest>(&encoded_identity).unwrap(),
        identity
    );
}

#[test]
fn deleted_app_action_envelope_does_not_deserialize() {
    let result: Result<WorkerRequest, _> = serde_json::from_value(json!({
        "type": "app_action",
        "correlation_id": "react-1",
        "action": { "action": "react", "target_event_id": "event-id" }
    }));
    assert!(
        result.is_err(),
        "the `app_action` envelope must not deserialize"
    );
}

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
        "the public JSON dispatch envelope is retired"
    );
}

#[test]
fn runtime_status_protocol_still_names_degraded_modes() {
    let event = WorkerEvent::RuntimeStatus {
        status: RuntimeStatus::Ready,
        correlation_id: Some("hello-1".to_string()),
    };
    let encoded = serde_json::to_string(&event).unwrap();
    assert_eq!(
        serde_json::from_str::<WorkerEvent>(&encoded).unwrap(),
        event
    );
}
