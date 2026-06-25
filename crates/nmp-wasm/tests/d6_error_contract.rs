//! D6 error-as-data contract tests for `RawWasmAbiAdapter::handle`.
//!
//! These tests verify that `RawWasmAbiAdapter::handle` never returns a
//! `WasmRuntimeError::InvalidConfig` when the host supplies valid-but-wrong
//! configuration, and that the `wasm_binding.rs` composition root has a clear
//! contract for which errors are protocol-layer data vs. catastrophic.
//!
//! The `WasmRuntimeError::InvalidConfig` variant is what `wasm_binding.rs`
//! converts to a `WorkerEvent::Error { code: "invalid_config" }` so the JS
//! host sees data instead of a Promise rejection. These tests verify that
//! `handle()` produces `InvalidConfig` for each bad-config case, giving the
//! binding layer a stable set of inputs to classify.

use nmp_wasm::{
    RawWasmAbiAdapter, RelayBootstrapEntry, StartConfig, WasmRuntimeError, WorkerEvent,
    WorkerRequest,
};

// ── InvalidConfig surfaces — each triggers a parse_error or invalid_config
//    WorkerEvent in the wasm_binding.rs D6 layer ──────────────────────────────

#[test]
fn start_with_empty_app_id_returns_invalid_config() {
    let mut runtime = RawWasmAbiAdapter::new();
    let result = runtime.handle(WorkerRequest::Start(StartConfig {
        app_id: "".to_string(),
        relays: vec!["wss://relay.example".to_string()],
        relay_bootstrap: vec![],
        database_name: "chirp-dev".to_string(),
        correlation_id: "start-1".to_string(),
    }));
    assert!(
        matches!(result, Err(WasmRuntimeError::InvalidConfig(_))),
        "empty app_id must produce InvalidConfig, got {result:?}"
    );
    if let Err(WasmRuntimeError::InvalidConfig(msg)) = result {
        assert!(
            msg.contains("app_id"),
            "message must name the failing field: {msg}"
        );
    }
}

#[test]
fn start_with_whitespace_only_app_id_returns_invalid_config() {
    let mut runtime = RawWasmAbiAdapter::new();
    let result = runtime.handle(WorkerRequest::Start(StartConfig {
        app_id: "   ".to_string(),
        relays: vec!["wss://relay.example".to_string()],
        relay_bootstrap: vec![],
        database_name: "chirp-dev".to_string(),
        correlation_id: "start-1".to_string(),
    }));
    assert!(
        matches!(result, Err(WasmRuntimeError::InvalidConfig(_))),
        "whitespace-only app_id must produce InvalidConfig, got {result:?}"
    );
}

#[test]
fn start_with_empty_database_name_returns_invalid_config() {
    let mut runtime = RawWasmAbiAdapter::new();
    let result = runtime.handle(WorkerRequest::Start(StartConfig {
        app_id: "chirp".to_string(),
        relays: vec!["wss://relay.example".to_string()],
        relay_bootstrap: vec![],
        database_name: "".to_string(),
        correlation_id: "start-1".to_string(),
    }));
    assert!(
        matches!(result, Err(WasmRuntimeError::InvalidConfig(_))),
        "empty database_name must produce InvalidConfig, got {result:?}"
    );
    if let Err(WasmRuntimeError::InvalidConfig(msg)) = result {
        assert!(
            msg.contains("database_name"),
            "message must name the failing field: {msg}"
        );
    }
}

#[test]
fn start_with_no_relays_returns_invalid_config() {
    let mut runtime = RawWasmAbiAdapter::new();
    let result = runtime.handle(WorkerRequest::Start(StartConfig {
        app_id: "chirp".to_string(),
        relays: vec![],
        relay_bootstrap: vec![],
        database_name: "chirp-dev".to_string(),
        correlation_id: "start-1".to_string(),
    }));
    assert!(
        matches!(result, Err(WasmRuntimeError::InvalidConfig(_))),
        "empty relay list must produce InvalidConfig, got {result:?}"
    );
    if let Err(WasmRuntimeError::InvalidConfig(msg)) = result {
        assert!(
            msg.contains("relay"),
            "message must reference the relay requirement: {msg}"
        );
    }
}

// ── D6 resolve-as-data contract — valid requests still surface errors as events,
//    not Err variants. These are regression guards for the boundary line. ───────

#[test]
fn protocol_mismatch_hello_resolves_as_error_event_not_err() {
    use nmp_wasm::ClientHello;
    let mut runtime = RawWasmAbiAdapter::new();
    // Version 2 does not exist; the runtime must resolve as WorkerEvent::Error
    // (data), not Err(WasmRuntimeError). This is the existing D6 path in
    // runtime.rs; this test is a regression guard.
    let result = runtime.handle(WorkerRequest::Hello(ClientHello {
        app_id: "chirp".to_string(),
        platform: "web".to_string(),
        protocol_version: 99,
    }));
    let events = result.expect("protocol mismatch must not return Err — it must be data");
    assert_eq!(events.len(), 1);
    match &events[0] {
        WorkerEvent::Error { code, .. } => {
            assert_eq!(code, "protocol_mismatch");
        }
        other => panic!("expected WorkerEvent::Error, got {other:?}"),
    }
}

#[test]
fn valid_start_still_succeeds_after_d6_refactor() {
    // Smoke test: a correct Start still works after the D6 changes in
    // wasm_binding.rs — the runtime's Ok path is unaffected.
    let mut runtime = RawWasmAbiAdapter::new();
    let result = runtime.handle(WorkerRequest::Start(StartConfig {
        app_id: "chirp".to_string(),
        relays: vec!["wss://relay.example".to_string()],
        relay_bootstrap: vec![RelayBootstrapEntry {
            url: "wss://relay.example".to_string(),
            role: "both".to_string(),
        }],
        database_name: "chirp-dev".to_string(),
        correlation_id: "start-ok".to_string(),
    }));
    let events = result.expect("valid Start must succeed");
    assert!(
        !events.is_empty(),
        "valid Start must produce at least one event"
    );
}
