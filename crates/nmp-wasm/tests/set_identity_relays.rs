use nmp_wasm::{
    IdentityRelayPermission, RawWasmAbiAdapter, RelayBootstrapEntry, SetIdentity, StartConfig,
    WorkerEvent, WorkerRequest,
};

#[test]
fn set_identity_merges_nip07_relays_before_snapshot() {
    let mut runtime = RawWasmAbiAdapter::new();
    runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec!["wss://relay.example".to_string()],
            relay_bootstrap: vec![RelayBootstrapEntry {
                url: "wss://relay.example".to_string(),
                role: "both".to_string(),
            }],
            database_name: "chirp-dev".to_string(),
            correlation_id: "start-1".to_string(),
        }))
        .unwrap();

    let events = runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "nip07".to_string(),
            pubkey_hex: "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
                .to_string(),
            correlation_id: "set-1".to_string(),
            identity_relays: vec![
                identity_relay("wss://read.example/", true, false),
                identity_relay("wss://write.example", false, true),
                identity_relay("wss://relay.example", true, false),
                identity_relay("https://not-a-relay.example", true, true),
            ],
        }))
        .unwrap();

    let bytes = events
        .iter()
        .find_map(|event| match event {
            WorkerEvent::UpdateBytes { bytes } => Some(bytes),
            _ => None,
        })
        .expect("set_identity must emit a snapshot");
    let projections =
        nmp_core::decode_snapshot_typed_projections(bytes).expect("snapshot must decode");
    let configured = projections
        .iter()
        .find(|entry| entry.schema_id == nmp_core::typed_projections::CONFIGURED_RELAYS_SCHEMA_ID)
        .expect("configured_relays sidecar must be present");
    let model = nmp_core::typed_projections::decode_configured_relays(&configured.payload)
        .expect("configured_relays must decode");

    let role = |url: &str| {
        model
            .relays
            .iter()
            .find(|row| row.url == url)
            .map(|row| row.role.as_str())
    };
    assert_eq!(role("wss://relay.example"), Some("both,indexer"));
    assert_eq!(role("wss://read.example"), Some("read,indexer"));
    assert_eq!(role("wss://write.example"), Some("both,indexer"));
    assert_eq!(role("https://not-a-relay.example"), None);

    assert!(
        runtime.maintenance_deadline_armed_for_test(),
        "identity relay merge must request an event drain for bootstrap recompilation",
    );
}

fn identity_relay(url: &str, read: bool, write: bool) -> IdentityRelayPermission {
    IdentityRelayPermission {
        url: url.to_string(),
        read,
        write,
    }
}
