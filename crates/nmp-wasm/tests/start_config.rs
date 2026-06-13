//! `StartConfig` relay-policy deserialization contract (#1125).
//!
//! Relay defaults are host policy, not framework policy: the nmp-wasm worker
//! protocol carries no built-in relay defaults, so a host that omits `relays`
//! or `relay_bootstrap` must fail deserialization loudly. Lives in its own
//! sibling file so `tests/protocol.rs` stays under the 500-LOC hard cap.

use nmp_wasm::{RelayBootstrapEntry, StartConfig, WorkerRequest};

#[test]
fn start_config_requires_host_supplied_relays() {
    // Relay policy is host policy, not framework policy (#1125). A host that
    // omits `relays` / `relay_bootstrap` MUST fail deserialization loudly —
    // the framework no longer silently substitutes one app's relay defaults.
    let missing_relays = serde_json::from_value::<WorkerRequest>(serde_json::json!({
        "type": "start",
        "app_id": "chirp",
        "relay_bootstrap": [{ "url": "wss://relay.example", "role": "both" }],
        "database_name": "chirp-dev",
        "correlation_id": "start-1"
    }));
    assert!(
        missing_relays.is_err(),
        "omitting `relays` must fail deserialization"
    );

    let missing_bootstrap = serde_json::from_value::<WorkerRequest>(serde_json::json!({
        "type": "start",
        "app_id": "chirp",
        "relays": ["wss://relay.example"],
        "database_name": "chirp-dev",
        "correlation_id": "start-1"
    }));
    assert!(
        missing_bootstrap.is_err(),
        "omitting `relay_bootstrap` must fail deserialization"
    );

    // Both supplied → deserialization succeeds.
    let decoded: WorkerRequest = serde_json::from_value(serde_json::json!({
        "type": "start",
        "app_id": "chirp",
        "relays": ["wss://relay.example"],
        "relay_bootstrap": [{ "url": "wss://relay.example", "role": "both,indexer" }],
        "database_name": "chirp-dev",
        "correlation_id": "start-1"
    }))
    .unwrap();
    assert_eq!(
        decoded,
        WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec!["wss://relay.example".to_string()],
            relay_bootstrap: vec![RelayBootstrapEntry {
                url: "wss://relay.example".to_string(),
                role: "both,indexer".to_string(),
            }],
            database_name: "chirp-dev".to_string(),
            correlation_id: "start-1".to_string(),
        })
    );
}
