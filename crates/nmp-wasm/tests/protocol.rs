use nmp_wasm::{
    ChirpAction, ChirpActionDispatch, ClientHello, DegradedMode, RuntimeStatus, StartConfig,
    WasmRuntime, WorkerEvent, WorkerRequest,
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
fn start_enters_explicit_degraded_mode_until_actor_driver_lands() {
    let mut runtime = WasmRuntime::new();

    let event = runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec!["wss://relay.example".to_string()],
            database_name: "chirp-dev".to_string(),
            correlation_id: "start-1".to_string(),
        }))
        .unwrap();

    assert_eq!(
        event,
        WorkerEvent::RuntimeStatus {
            status: RuntimeStatus::Degraded(DegradedMode::BrowserActorDriverMissing),
            correlation_id: Some("start-1".to_string()),
        }
    );
}

#[test]
fn invalid_protocol_is_rejected_before_start() {
    let mut runtime = WasmRuntime::new();

    let event = runtime
        .handle(WorkerRequest::Hello(ClientHello {
            app_id: "chirp".to_string(),
            platform: "web".to_string(),
            protocol_version: 2,
        }))
        .unwrap();

    match event {
        WorkerEvent::Error { code, .. } => assert_eq!(code, "protocol_mismatch"),
        other => panic!("expected protocol error, got {other:?}"),
    }
}

#[test]
fn chirp_action_publish_note_maps_to_kernel_publish_action() {
    let action = ChirpActionDispatch {
        action: ChirpAction::PublishNote {
            content: "hello from web".to_string(),
            reply_to_id: None,
        },
        correlation_id: "pub-1".to_string(),
    }
    .into_action_dispatch();

    assert_eq!(action.action_type, "nmp.publish");
    assert_eq!(
        action.payload,
        json!({
            "PublishNote": {
                "content": "hello from web",
                "reply_to_id": null,
                "target": "Auto",
            }
        })
    );
}

#[test]
fn chirp_action_react_defaults_to_like_without_host_policy() {
    let request: WorkerRequest = serde_json::from_value(json!({
        "type": "chirp_action",
        "correlation_id": "react-1",
        "action": {
            "action": "react",
            "target_event_id": "event-id"
        }
    }))
    .unwrap();

    let WorkerRequest::ChirpAction(action) = request else {
        panic!("expected chirp action request");
    };
    let dispatch = action.into_action_dispatch();

    assert_eq!(dispatch.action_type, "chirp.react");
    assert_eq!(
        dispatch.payload,
        json!({
            "target_event_id": "event-id",
            "reaction": "+"
        })
    );
}

#[test]
fn chirp_action_uses_same_generic_worker_event_path() {
    let mut runtime = WasmRuntime::new();

    let event = runtime
        .handle(WorkerRequest::ChirpAction(ChirpActionDispatch {
            action: ChirpAction::Follow {
                pubkey: "deadbeef".to_string(),
            },
            correlation_id: "follow-1".to_string(),
        }))
        .unwrap();

    match event {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "chirp.follow");
            assert_eq!(failure.correlation_id, "follow-1");
        }
        other => panic!("expected degraded dispatch failure, got {other:?}"),
    }
}
