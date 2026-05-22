use nmp_wasm::{
    ChirpAction, ChirpActionDispatch, ClientHello, RelayBootstrapEntry, RuntimeStatus, StartConfig,
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
fn start_config_uses_shared_chirp_defaults_when_web_omits_relays() {
    let decoded: WorkerRequest = serde_json::from_value(serde_json::json!({
        "type": "start",
        "app_id": "chirp",
        "database_name": "chirp-dev",
        "correlation_id": "start-1"
    }))
    .unwrap();

    assert_eq!(
        decoded,
        WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: nmp_chirp_config::chirp_default_relay_urls(),
            relay_bootstrap: nmp_chirp_config::chirp_default_relay_bootstrap()
                .into_iter()
                .map(Into::into)
                .collect(),
            database_name: "chirp-dev".to_string(),
            correlation_id: "start-1".to_string(),
        })
    );
}

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
    assert!(matches!(events[1], WorkerEvent::Update { .. }));
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

    assert_eq!(dispatch.action_type, "nmp.nip25.react");
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

    let events = runtime
        .handle(WorkerRequest::ChirpAction(ChirpActionDispatch {
            action: ChirpAction::Follow {
                pubkey: "deadbeef".to_string(),
            },
            correlation_id: "follow-1".to_string(),
        }))
        .unwrap();

    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.follow");
            assert_eq!(failure.correlation_id, "follow-1");
        }
        other => panic!("expected degraded dispatch failure, got {other:?}"),
    }
}

#[test]
fn browser_publish_intent_emits_rust_owned_chirp_snapshot() {
    let mut runtime = WasmRuntime::new();
    runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: nmp_chirp_config::chirp_default_relay_urls(),
            relay_bootstrap: nmp_chirp_config::chirp_default_relay_bootstrap()
                .into_iter()
                .map(Into::into)
                .collect(),
            database_name: "chirp-dev".to_string(),
            correlation_id: "start-1".to_string(),
        }))
        .unwrap();

    let events = runtime
        .handle(WorkerRequest::ChirpAction(ChirpActionDispatch {
            action: ChirpAction::PublishNote {
                content: "hello from web".to_string(),
                reply_to_id: None,
            },
            correlation_id: "pub-1".to_string(),
        }))
        .unwrap();

    assert_eq!(
        events[0],
        WorkerEvent::ActionAccepted {
            action_type: "nmp.publish".to_string(),
            correlation_id: "pub-1".to_string(),
        }
    );
    let WorkerEvent::Update { envelope } = &events[1] else {
        panic!("expected update envelope, got {:?}", events[1]);
    };
    assert_eq!(
        envelope["chirpTimeline"]["cards"][0]["content"],
        "hello from web"
    );
    assert_eq!(
        envelope["v"]["projections"]["relay_diagnostics"][0]["url"],
        nmp_chirp_config::CHIRP_CONTENT_RELAY_URL
    );
    assert_eq!(
        envelope["v"]["projections"]["relay_diagnostics"][0]["role"],
        "both,indexer"
    );
}

#[test]
fn browser_publish_validation_lives_in_rust_facade() {
    let mut runtime = WasmRuntime::new();

    let events = runtime
        .handle(WorkerRequest::ChirpAction(ChirpActionDispatch {
            action: ChirpAction::PublishNote {
                content: "   ".to_string(),
                reply_to_id: None,
            },
            correlation_id: "pub-empty".to_string(),
        }))
        .unwrap();

    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.publish");
            assert_eq!(failure.correlation_id, "pub-empty");
            assert_eq!(failure.reason, "publish note content is empty");
        }
        other => panic!("expected Rust-side validation failure, got {other:?}"),
    }
}
