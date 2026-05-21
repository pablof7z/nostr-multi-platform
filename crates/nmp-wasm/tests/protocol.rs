use nmp_wasm::{
    ClientHello, DegradedMode, RuntimeStatus, StartConfig, WasmRuntime, WorkerEvent, WorkerRequest,
};

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
fn start_config_uses_core_chirp_defaults_when_web_omits_relays() {
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
            database_name: "chirp-dev".to_string(),
            correlation_id: "start-1".to_string(),
        })
    );
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
