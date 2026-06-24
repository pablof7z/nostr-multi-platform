// Native-only scheduler tests. The production wasm target schedules the same
// runtime deadline state with a one-shot browser timeout.
#![cfg(not(target_arch = "wasm32"))]

use std::fs;
use std::path::PathBuf;

use nmp_core::RelayRole;
use nmp_wasm::{RelayBootstrapEntry, StartConfig, WasmRuntime, WorkerRequest};

const RELAY_URL: &str = "wss://relay.example";

fn started_runtime() -> WasmRuntime {
    let mut runtime = WasmRuntime::new();
    runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec![RELAY_URL.to_string()],
            relay_bootstrap: vec![RelayBootstrapEntry {
                url: RELAY_URL.to_string(),
                role: "both".to_string(),
            }],
            database_name: "scheduler-test".to_string(),
            correlation_id: "start-1".to_string(),
        }))
        .expect("Start must succeed");
    runtime
}

#[test]
fn idle_runtime_deadline_fires_once_and_does_not_rearm() {
    let mut rt = started_runtime();

    assert!(
        rt.maintenance_deadline_armed_for_test(),
        "Start arms a single post-start maintenance deadline"
    );

    let (outbound, dirty) = rt
        .fire_maintenance_deadline_for_test()
        .expect("the post-start deadline must be armed");
    assert!(
        outbound.is_empty(),
        "idle deadline must produce no outbound"
    );
    assert!(!dirty, "idle deadline must not mark a snapshot dirty");
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "idle runtime must not re-arm a fixed cadence"
    );
    assert_eq!(rt.maintenance_deadline_fires_for_test(), 1);

    assert!(
        rt.fire_maintenance_deadline_for_test().is_none(),
        "no second wake exists without a new event or tracked deadline"
    );
}

#[test]
fn deadline_with_both_role_relay_claims_expansion_does_not_panic() {
    let mut runtime = started_runtime();

    runtime.inject_relay_connected_for_test(RelayRole::Content, RELAY_URL);

    let _ = runtime
        .fire_maintenance_deadline_for_test()
        .expect("relay-connected event must arm a deadline");
}

#[test]
fn relay_event_deadline_signals_dirty_snapshot_then_coalesces() {
    let mut rt = started_runtime();

    let _ = rt.snapshot_bytes_for_test();
    rt.inject_relay_connected_for_test(RelayRole::Content, RELAY_URL);

    let (_, dirty_after_connect) = rt
        .fire_maintenance_deadline_for_test()
        .expect("relay-connected event must arm a deadline");
    assert!(
        dirty_after_connect,
        "deadline after a relay-connected mutation must signal dirty"
    );

    let _ = rt.snapshot_bytes_for_test();
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "snapshot-cleared relay event must not leave an idle cadence armed"
    );
}

#[test]
fn production_scheduler_source_has_no_interval_driver() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for relative in ["Cargo.toml", "src/lib.rs", "src/runtime.rs", "src/tick.rs"] {
        let path = manifest.join(relative);
        let source = fs::read_to_string(&path).expect("scheduler source must be readable");
        for forbidden in [
            "Interval::new",
            "start_tick_interval",
            "tick_interval",
            "setInterval",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} must not reintroduce fixed interval polling token `{}`",
                path.display(),
                forbidden
            );
        }
    }
}
