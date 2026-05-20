//! Headless proof that the desktop shell's in-process kernel bring-up works:
//! spawn the actor, `Start`, and confirm a live feed arrives from the public
//! relay — the exact path `bridge::KernelBridge::start` drives, minus egui.
//!
//! Network-dependent, so `#[ignore]` by default (run with `--ignored` for
//! evidence). Mirrors the shell's snapshot contract via `serde_json::Value`
//! (the binary's `snapshot` module is not importable from an integration test).

use std::time::{Duration, Instant};

use nmp_core::testing::{spawn_actor, ActorCommand};
use nmp_core::UpdateEnvelope;

#[test]
#[ignore = "hits the public relay wss://relay.primal.net; run with --ignored"]
fn in_process_kernel_renders_live_feed() {
    let (tx, rx) = spawn_actor();
    tx.send(ActorCommand::Start {
        visible_limit: 80,
        emit_hz: 4,
    })
    .expect("actor accepts Start");

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut best_items = 0usize;
    let mut best_events = 0u64;

    while Instant::now() < deadline {
        let Ok(line) = rx.recv_timeout(Duration::from_secs(5)) else {
            continue;
        };
        let Ok(env) = serde_json::from_str::<UpdateEnvelope>(&line) else {
            continue;
        };
        let UpdateEnvelope::FullState(v) = env else {
            continue;
        };
        let items = v["items"].as_array().map(Vec::len).unwrap_or(0);
        let events = v["metrics"]["events_rx"].as_u64().unwrap_or(0);
        best_items = best_items.max(items);
        best_events = best_events.max(events);
        if best_items > 0 {
            break;
        }
    }

    let _ = tx.send(ActorCommand::Shutdown);

    println!("live feed: items={best_items} events_rx={best_events}");
    assert!(
        best_items > 0,
        "expected a live timeline from wss://relay.primal.net within 30s \
         (items={best_items}, events_rx={best_events})"
    );
}
