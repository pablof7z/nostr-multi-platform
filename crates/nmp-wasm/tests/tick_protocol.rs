// This test uses native-only helpers (inject_relay_connected_for_test,
// tick_for_test, snapshot_bytes_for_test) that only exist under
// #[cfg(not(target_arch = "wasm32"))].  Skip the entire file when compiled
// for wasm32 so `wasm-pack test` does not abort the build.
#![cfg(not(target_arch = "wasm32"))]

// PR-2 acceptance: worker tick driver — idle-tick coalescing.
//
// Proves that `tick_for_test` (which calls the same `tick::tick_once` core
// the wasm32 1 Hz timer closure calls) does NOT signal a snapshot push when
// the kernel state has not changed since the last `make_update_frame`.
//
// Three-phase coalescing proof:
//   1. After `Start`, take a snapshot (clears `changed_since_emit`).
//   2. Idle tick → `dirty == false`; no snapshot push would occur.
//   3. Inject a relay-connected event (mutates kernel state → dirty = true).
//   4. Verify that a non-idle tick does signal a push (dirty = true).
//   5. Take a snapshot to clear dirty again.
//   6. Another idle tick → `dirty == false` again; coalescing re-armed.
//
// The coalescing gate in the real wasm32 timer is `if dirty { push... }`.
// If that gate were absent every tick would burn a JS heap allocation + an
// upstream re-render regardless of whether anything had changed.

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
            database_name: "tick-test".to_string(),
            correlation_id: "start-1".to_string(),
        }))
        .expect("Start must succeed");
    runtime
}

#[test]
fn idle_tick_does_not_signal_snapshot_push() {
    let mut rt = started_runtime();

    // Phase 1: take a snapshot — clears changed_since_emit.
    let _ = rt.snapshot_bytes_for_test();

    // Phase 2: idle tick (nothing pending after a fresh Start + snapshot).
    let (outbound, dirty) = rt.tick_for_test();
    assert!(outbound.is_empty(), "idle tick must produce no outbound");
    assert!(
        !dirty,
        "idle tick must not signal a snapshot push (dirty-flag coalescing)"
    );
}

#[test]
fn tick_after_relay_event_signals_push_then_coalesces() {
    let mut rt = started_runtime();

    // Take baseline snapshot to clear dirty flag.
    let _ = rt.snapshot_bytes_for_test();

    // Phase 3: inject relay-connected — this mutates kernel relay state.
    rt.inject_relay_connected_for_test(RelayRole::Content, RELAY_URL);

    // Phase 4: tick should be dirty because the relay state changed.
    // (inject_relay_connected calls handle_relay_connected which marks the
    // kernel dirty via the usual mutation path.)
    let (_, dirty_after_connect) = rt.tick_for_test();
    assert!(
        dirty_after_connect,
        "tick after a relay-connected mutation must signal dirty (snapshot push required)"
    );

    // Phase 5: take a snapshot — clears dirty.
    let _ = rt.snapshot_bytes_for_test();

    // Phase 6: another idle tick — no new mutations → not dirty again.
    let (outbound2, dirty2) = rt.tick_for_test();
    let _ = outbound2;
    assert!(
        !dirty2,
        "idle tick after snapshot must not signal dirty — coalescing must re-arm"
    );
}
