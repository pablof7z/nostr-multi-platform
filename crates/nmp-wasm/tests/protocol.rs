use nmp_wasm::{
    AppAction, AppActionDispatch, ClientHello, RelayBootstrapEntry, RuntimeStatus, SetSigner,
    StartConfig, WasmRuntime, WorkerEvent, WorkerRequest,
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
            // Stage 3b: no signer installed → `signer_not_installed`. The
            // host pattern-matches on this prefix to surface a "sign in to
            // publish" banner.
            assert!(
                failure.reason.starts_with("signer_not_installed"),
                "expected signer_not_installed prefix, got: {}",
                failure.reason
            );
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
fn publish_note_without_signer_returns_signer_not_installed() {
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

    // V-01 Stage 3b: with no signer installed, app-level writes fail with
    // `signer_not_installed` (more precise than the Stage 3
    // `browser_actor_driver_missing` blanket — see runtime.rs comment for
    // the two-state model).
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
fn publish_note_after_set_signer_returns_publish_path_not_wired() {
    let mut runtime = WasmRuntime::new();

    // Install a nip07 signer with a real (test-fixture) pubkey hex. On
    // native this constructs a stub that returns `Unsupported` from sign();
    // we don't reach sign() in this assertion — the runtime stops at the
    // publish-path-not-wired error because Stage 3b ships the signer slot,
    // not the publish path.
    let set_events = runtime
        .handle(WorkerRequest::SetSigner(SetSigner {
            kind: "nip07".to_string(),
            pubkey_hex:
                "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
                    .to_string(),
            correlation_id: "set-1".to_string(),
        }))
        .unwrap();
    match &set_events[0] {
        WorkerEvent::ActionAccepted { action_type, correlation_id } => {
            assert_eq!(action_type, "nmp.set_signer");
            assert_eq!(correlation_id, "set-1");
        }
        other => panic!("expected ActionAccepted, got {other:?}"),
    }

    // Now the same app-level write surfaces the *second* honest error: the
    // signer is installed but the SYNCHRONOUS publish path is not wired —
    // V-01 Stage 3c added an asynchronous publish entrypoint
    // (`NmpWasmRuntime::dispatch_app_action_async(...)`) which the message
    // points hosts at. Hosts can distinguish "you need to sign in" from
    // "use the async entrypoint" by pattern-matching the reason prefix.
    let events = runtime
        .handle(WorkerRequest::AppAction(AppActionDispatch {
            action: AppAction::PublishNote {
                content: "hello from web".to_string(),
                reply_to_id: None,
            },
            correlation_id: "pub-1".to_string(),
        }))
        .unwrap();
    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.publish");
            assert!(
                failure.reason.starts_with("publish_path_not_wired"),
                "expected publish_path_not_wired prefix, got: {}",
                failure.reason
            );
            // V-01 Stage 3c contract: the failure reason MUST point hosts at
            // the new async entrypoint so the integration is self-documenting.
            // A host that pattern-matches on the prefix already knows what to
            // do, but a developer reading the reason string in DevTools should
            // see exactly which method to call.
            assert!(
                failure.reason.contains("dispatch_app_action_async"),
                "expected reason to point host at the async entrypoint, got: {}",
                failure.reason
            );
        }
        other => panic!("expected CapabilityFailure, got {other:?}"),
    }
}

#[test]
fn set_signer_with_unknown_kind_returns_unsupported_signer_kind() {
    let mut runtime = WasmRuntime::new();
    let events = runtime
        .handle(WorkerRequest::SetSigner(SetSigner {
            kind: "magic".to_string(),
            pubkey_hex: String::new(),
            correlation_id: "set-1".to_string(),
        }))
        .unwrap();
    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.set_signer");
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
fn set_signer_with_garbage_hex_returns_invalid_signer_pubkey() {
    let mut runtime = WasmRuntime::new();
    let events = runtime
        .handle(WorkerRequest::SetSigner(SetSigner {
            kind: "nip07".to_string(),
            pubkey_hex: "not-hex".to_string(),
            correlation_id: "set-1".to_string(),
        }))
        .unwrap();
    match &events[0] {
        WorkerEvent::CapabilityFailure(failure) => {
            assert_eq!(failure.capability, "nmp.set_signer");
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
fn set_signer_serde_round_trip_through_json() {
    // The wasm-bindgen `handle_json` entry point deserialises every
    // WorkerRequest from JSON, so the SetSigner variant must round-trip
    // through serde with the snake-cased tag the JS host sends.
    let request: WorkerRequest = serde_json::from_value(json!({
        "type": "set_signer",
        "kind": "nip07",
        "pubkey_hex": "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
        "correlation_id": "set-1",
    }))
    .unwrap();

    match request {
        WorkerRequest::SetSigner(set) => {
            assert_eq!(set.kind, "nip07");
            assert_eq!(set.correlation_id, "set-1");
        }
        other => panic!("expected SetSigner, got {other:?}"),
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
            payload: serde_json::json!({"PublishNote": {"content": "hi", "target": "Auto"}}),
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
