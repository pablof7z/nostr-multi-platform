use nmp_wasm::{
    AppAction, AppActionDispatch, ClientHello, RelayBootstrapEntry, RuntimeStatus, StartConfig,
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
    let action = AppActionDispatch {
        action: AppAction::PublishNote {
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

    let WorkerRequest::AppAction(action) = request else {
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
        .handle(WorkerRequest::AppAction(AppActionDispatch {
            action: AppAction::Follow {
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

// V-01 Stage 2: `WasmRuntime` no longer keeps a local `Vec<LocalNote>` and no
// longer fabricates a snapshot that "contains" the published note. The pure
// `KernelReducer` runs in WASM, but the actor + relay_worker (and therefore
// every signed-event publish path) are `#[cfg(feature = "native")]` and
// unreachable. The honest contract for app-level intents in browser WASM
// today is `CapabilityFailure(browser_actor_driver_missing)`; Stage 3 will
// wire `web_sys::WebSocket` so these complete.

#[test]
fn start_emits_canonical_snapshot_envelope_from_real_kernel() {
    let mut runtime = WasmRuntime::new();
    let events = runtime
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

    assert_eq!(events.len(), 2, "Start must emit RuntimeStatus + Update");
    let WorkerEvent::Update { envelope } = &events[1] else {
        panic!("expected update envelope, got {:?}", events[1]);
    };

    // Envelope is the canonical `wrap_snapshot` shape every native host
    // also decodes: `{"t":"snapshot","v":{…}}`. No more bespoke "chirpTimeline"
    // synthesized field — that was the stub leaking app-noun shape into the
    // wire envelope.
    assert_eq!(envelope["t"], "snapshot");
    let payload = &envelope["v"];

    // The inner payload's `rev` is the kernel's own rev, not a runtime-local
    // counter. `KernelAction::Start` always returns `Started { rev: 0 }` on a
    // fresh kernel, so the wasm runtime mirrors that.
    assert_eq!(payload["rev"], 0, "rev must match KernelUpdate::Started");
    assert_eq!(payload["running"], true);
    assert_eq!(payload["database_name"], "chirp-dev");
    assert_eq!(payload["schema_version"], 1);

    // `relay_diagnostics` carries the bootstrap entries the host supplied at
    // Start time. Status is "configured" — the honest state until Stage 3
    // (web_sys::WebSocket) connects.
    let diags = &payload["projections"]["relay_diagnostics"];
    assert!(diags.is_array(), "relay_diagnostics must be an array");
    assert_eq!(
        diags[0]["url"],
        nmp_chirp_config::CHIRP_CONTENT_RELAY_URL
    );
    assert_eq!(diags[0]["role"], "both,indexer");
    assert_eq!(diags[0]["status"], "configured");
}

#[test]
fn publish_note_returns_browser_driver_missing_until_stage_3() {
    let mut runtime = WasmRuntime::new();

    let events = runtime
        .handle(WorkerRequest::AppAction(AppActionDispatch {
            action: AppAction::PublishNote {
                content: "hello from web".to_string(),
                reply_to_id: None,
            },
            correlation_id: "pub-1".to_string(),
        }))
        .unwrap();

    // No actor → no publish path. Honest failure rather than fabricated
    // success. The reason carries a stable prefix host UIs can pattern-match
    // for the degraded-mode banner.
    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.publish");
            assert_eq!(failure.correlation_id, "pub-1");
            assert!(
                failure.reason.starts_with("browser_actor_driver_missing"),
                "expected browser_actor_driver_missing prefix, got: {}",
                failure.reason
            );
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
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
    let WorkerEvent::Update { envelope } = &events[1] else {
        panic!("expected update envelope, got {:?}", events[1]);
    };
    // Real kernel `rev` came back through `KernelUpdate::Started`. Stub never
    // touched the kernel at all, so this assertion failing means a regression
    // back to the synthetic-JSON path.
    assert_eq!(envelope["v"]["rev"], 0);
    assert_eq!(envelope["v"]["running"], true);
}

#[test]
fn app_namespaced_dispatch_returns_browser_driver_missing() {
    use nmp_wasm::ActionDispatch;
    let mut runtime = WasmRuntime::new();

    // `nmp.publish` is an *app* action — it produces a signed event. Without
    // the actor + relay worker this cannot complete; the runtime returns
    // CapabilityFailure rather than fabricating a snapshot the way the stub
    // did.
    let events = runtime
        .handle(WorkerRequest::Dispatch(ActionDispatch {
            action_type: "nmp.publish".to_string(),
            payload: serde_json::json!({"PublishNote": {"content": "hi", "target": "Auto"}}),
            correlation_id: "pub-2".to_string(),
        }))
        .unwrap();

    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.publish");
            assert!(failure.reason.starts_with("browser_actor_driver_missing"));
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}
