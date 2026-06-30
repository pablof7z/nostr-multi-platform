//! Live profile-thrashing scenario.
//!
//! Simulates scroll-driven mount/unmount of profile interests at 50/sec for
//! 10 minutes against the real kernel actor.
//!
//! Gates (M1 exit):
//! - `dedup_ratio` (new wire subs ÷ mount cycles) ≤ 0.01
//! - `snapshot_valid` — final snapshot must be present and parsable (P3 fix:
//!   prevents false-pass when snapshot stream is cut off before leak gate runs)
//! - `leaked_subscriptions` = 0 after all claims released
//! - `relay_connected` = 1 (gate is only meaningful with a live relay)
//!
//! The spec's "OpenView/CloseView dispatch rate ≤ 60 % of mount rate" refers
//! to the platform debounce layer, deferred to M14 per T22.  This scenario
//! verifies the kernel-side dedup invariant only.

use super::{drain_until, open_sub_count, visible_items, wait_connected, wait_update, Scenario};
use crate::report::ScenarioMetrics;
use crate::scenarios::{finish_scenario, gate_eq, gate_max, gate_min};
use nmp_core::actor::{LifecycleCommand, RefsCommand};
use nmp_core::testing::{spawn_actor, ActorCommand};
use nmp_core::{ProfileShape, RefLiveness, RefNamespace, RefShape};
use nmp_testing::harness_probe::recv_latest_until;
use std::time::{Duration, Instant};

const DEDUP_RATIO_GATE: f64 = 0.01;
const THRASHING_DURATION: Duration = Duration::from_secs(600);
const THRASHING_RATE: f64 = 50.0;

const SEED_PUBKEYS: &[&str] = &[
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52",
    "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
    "32e1827635450ebb3c5a7d12c1f8e7b2b514439ac10a67eef3d9fd9c5c68e245",
];

pub(crate) fn profile_thrashing() -> Scenario {
    let wall_start = Instant::now();
    let (tx, rx) = spawn_actor();
    let _ = tx.send(ActorCommand::Lifecycle(LifecycleCommand::Start {
        visible_limit: 80,
        emit_hz: 4,
        initial_relays: Vec::new(),
    }));

    let connected = wait_connected(&rx);

    // Wait for the first timeline item so the seed-timeline REQ is already
    // counted in the baseline before thrashing begins.
    let mut baseline_subs = 0usize;
    if connected {
        let warmup_deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let Some(update) = wait_update(&rx, warmup_deadline) else {
                break;
            };
            let visible = visible_items(&update).unwrap_or(0) as usize;
            if visible >= 1 {
                baseline_subs = open_sub_count(&update);
                break;
            }
            if Instant::now() >= warmup_deadline {
                break;
            }
        }
    }

    let mut mount_count: u64 = 0;
    let mut dispatch_count: u64 = 0;

    if connected {
        let interval = Duration::from_secs_f64(1.0 / THRASHING_RATE);
        let end = Instant::now() + THRASHING_DURATION;
        let pool_size = SEED_PUBKEYS.len();
        let mut tick = Instant::now();
        let mut slot: usize = 0;

        while Instant::now() < end {
            if Instant::now() >= tick {
                let pubkey = SEED_PUBKEYS[slot % pool_size];
                let consumer = format!("bench-{}", slot % 20);

                let _ = tx.send(ActorCommand::Refs(RefsCommand::Resolve {
                    namespace: RefNamespace::Profile,
                    key: pubkey.to_string(),
                    consumer_id: consumer.clone(),
                    shape: RefShape::Profile(ProfileShape::Card),
                    liveness: RefLiveness::CacheOk,
                    force: false,
                    hints: Vec::new(),
                }));
                let _ = tx.send(ActorCommand::Refs(RefsCommand::Release {
                    namespace: RefNamespace::Profile,
                    key: pubkey.to_string(),
                    consumer_id: consumer,
                }));
                mount_count += 1;
                dispatch_count += 2;

                slot = slot.wrapping_add(1);
                tick += interval;
            }
            // Block until the next tick deadline or until a frame arrives —
            // event-driven (D8): wakes exactly when the actor pushes a snapshot,
            // never on a fixed wall-clock tick.
            recv_latest_until(&rx, tick);
        }
    }

    // Wait up to 5 s for the actor to push its final snapshot after the burst
    // quiesces.  The old 300 ms sleep raced the actor: drain() would return
    // None if the snapshot hadn't arrived yet, triggering the usize::MAX
    // sentinel and a false overall_passed=false (run 1779077038-live).
    let final_update = drain_until(&rx, Duration::from_secs(5));

    // Guard: a missing or unparsable final snapshot must fail the gate rather
    // than silently saturating-sub to zero (false-pass risk per codex T23/P3).
    let snapshot_valid = final_update
        .as_deref()
        .map(|bytes| nmp_core::decode_snapshot_envelope(bytes).is_ok())
        .unwrap_or(false);
    let final_subs = if snapshot_valid {
        open_sub_count(final_update.as_deref().unwrap_or(&[]))
    } else {
        // Return a sentinel so saturating_sub reports a large leak value,
        // ensuring the gate fails rather than silently passing.
        usize::MAX
    };

    let _ = tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown));

    let total_ops = mount_count * 2;

    // Note: if final_subs == usize::MAX the snapshot was invalid; the
    // saturating_sub still produces a large value that fails the gate.
    let new_subs_during_thrashing = final_subs.saturating_sub(baseline_subs) as u64;

    let dedup_ratio = if mount_count > 0 {
        new_subs_during_thrashing as f64 / mount_count as f64
    } else {
        0.0
    };

    let metrics = ScenarioMetrics {
        mount_unmount_rate_per_sec: Some(THRASHING_RATE),
        leaked_subscriptions: Some(new_subs_during_thrashing),
        trace_records: Some(total_ops),
        synthetic_runtime_ms: Some(wall_start.elapsed().as_millis()),
        ..ScenarioMetrics::default()
    };

    finish_scenario(
        "profile_thrashing",
        "Live: claim/release at 50/sec for 10 min against the real kernel actor.",
        THRASHING_DURATION.as_secs(),
        total_ops,
        metrics,
        vec![
            // Snapshot must be present/parsable or leak gate is meaningless.
            gate_min(
                "snapshot_valid",
                if snapshot_valid { 1.0 } else { 0.0 },
                1.0,
                Some("final kernel snapshot must be present and parsable".to_string()),
            ),
            gate_max(
                "dedup_ratio",
                dedup_ratio,
                DEDUP_RATIO_GATE,
                Some("new wire subs / mount cycles; kernel dedup keeps this near zero".to_string()),
            ),
            gate_eq(
                "leaked_subscriptions",
                new_subs_during_thrashing,
                0,
                Some("profile-claim REQs are deduped; zero net new subs expected".to_string()),
            ),
            gate_min(
                "relay_connected",
                if connected { 1.0 } else { 0.0 },
                1.0,
                Some("relay must be connected for leak gate to be meaningful".to_string()),
            ),
        ],
        vec![
            format!("mount_cycles={mount_count}"),
            format!("dispatch_count={dispatch_count}"),
            format!("dedup_ratio={dedup_ratio:.6}"),
            format!("baseline_wire_subs={baseline_subs} final_wire_subs={final_subs}"),
            format!("connected={connected}"),
            format!("snapshot_valid={snapshot_valid}"),
            "hardware=M3_Max developer machine".to_string(),
            "note: kernel dedup REQs; seed pubkeys pre-loaded at startup".to_string(),
            "note: dispatch_rate gate (≤60% of mount rate) deferred to M14/T22".to_string(),
        ],
    )
}
