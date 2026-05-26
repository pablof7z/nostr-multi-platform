//! Kernel-level integration tests for the `action_stages` projection.
//!
//! Covers the contract the FFI seam relies on:
//!
//! 1. `record_action_stage` appends to the actor-owned tracker and flips
//!    `changed_since_emit` so the next tick emits.
//! 2. The snapshot mirror is a *copy*, not a drain — the same entry
//!    appears on every tick until acked. This is the race-protection
//!    guarantee codex insisted on.
//! 3. `ack_action_stage` drops the entry, and the next tick's projection
//!    omits it.
//! 4. The `at_ms` timestamp routes through the injected `Clock` so a
//!    `FixedClock` makes the recorded history deterministic.

use super::action_stages::ActionStage;
use super::Kernel;

/// Build a kernel for unit tests. Uses the same `new(visible_limit)`
/// constructor every other kernel-level test uses.
fn kernel() -> Kernel {
    Kernel::new(64)
}

/// Pull the `action_stages` projection out of the kernel's snapshot JSON.
/// Returns `None` when the projection is absent (steady state). Mirrors the
/// `snapshot_registry_tests.rs` pattern — drive `make_update(true)` and read
/// out of the `projections` map.
fn stages_proj(kernel: &mut Kernel) -> Option<serde_json::Value> {
    let snapshot_json = kernel.make_update_json_for_test(true);
    let snap: serde_json::Value = serde_json::from_str(&snapshot_json).expect("update JSON parses");
    snap.get("projections")
        .and_then(|p| p.get("action_stages"))
        .cloned()
}

#[test]
fn record_action_stage_appears_in_snapshot() {
    let mut k = kernel();
    k.record_action_stage("corr-a", ActionStage::Requested, None);
    let proj = stages_proj(&mut k).expect("projection emitted after record");
    let entry = &proj["corr-a"];
    let arr = entry.as_array().expect("history is an array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["stage"], "requested");
}

#[test]
fn snapshot_persists_across_multiple_ticks() {
    // The load-bearing race-protection guarantee: an unacked entry survives
    // every subsequent snapshot emit. Without this, a host that missed one
    // tick could lose the terminal stage forever.
    let mut k = kernel();
    k.record_action_stage("corr-persist", ActionStage::Requested, None);
    k.record_action_stage("corr-persist", ActionStage::Accepted, None);
    let snap_a = stages_proj(&mut k).expect("first emit carries the entry");
    let snap_b = stages_proj(&mut k).expect("second emit still carries it");
    let snap_c = stages_proj(&mut k).expect("third emit too");
    assert_eq!(snap_a, snap_b);
    assert_eq!(snap_b, snap_c);
    let arr = snap_c["corr-persist"].as_array().unwrap();
    assert_eq!(arr.len(), 2, "both stages survive across ticks");
}

#[test]
fn ack_drops_entry_from_subsequent_snapshots() {
    let mut k = kernel();
    k.record_action_stage("corr-ack", ActionStage::Requested, None);
    k.record_action_stage("corr-ack", ActionStage::Accepted, None);
    assert!(stages_proj(&mut k).is_some());
    k.ack_action_stage("corr-ack");
    let after = stages_proj(&mut k);
    assert!(
        after.is_none(),
        "after ack the projection is steady-state-empty, so the key is omitted"
    );
}

#[test]
fn ack_of_unknown_correlation_is_silent_noop() {
    let mut k = kernel();
    // Acking an id that was never recorded must not crash and must leave the
    // tracker in steady state.
    k.ack_action_stage("never-existed");
    let after = stages_proj(&mut k);
    assert!(after.is_none());
}

#[test]
fn multiple_correlations_emit_independently() {
    let mut k = kernel();
    k.record_action_stage("corr-1", ActionStage::Requested, None);
    k.record_action_stage("corr-2", ActionStage::Publishing, None);
    let proj = stages_proj(&mut k).unwrap();
    assert!(proj.get("corr-1").is_some());
    assert!(proj.get("corr-2").is_some());

    // Ack one — the other survives.
    k.ack_action_stage("corr-1");
    let proj2 = stages_proj(&mut k).unwrap();
    assert!(proj2.get("corr-1").is_none());
    assert!(proj2.get("corr-2").is_some());
}

#[test]
fn detail_payload_round_trips() {
    let mut k = kernel();
    k.record_action_stage(
        "corr-d",
        ActionStage::Publishing,
        Some(serde_json::json!({ "relay": "wss://r.example", "attempt": 1 })),
    );
    let proj = stages_proj(&mut k).unwrap();
    let entry = &proj["corr-d"][0];
    assert_eq!(entry["detail"]["relay"], "wss://r.example");
    assert_eq!(entry["detail"]["attempt"], 1);
}

#[test]
fn failed_stage_carries_reason_into_snapshot() {
    let mut k = kernel();
    k.record_action_stage(
        "corr-f",
        ActionStage::Failed {
            reason: "no relays settled".to_string(),
        },
        None,
    );
    let proj = stages_proj(&mut k).unwrap();
    let entry = &proj["corr-f"][0];
    assert_eq!(entry["stage"], "failed");
    assert_eq!(entry["reason"], "no relays settled");
}

#[test]
fn at_ms_routes_through_clock_seam() {
    // The `at_ms` field is sourced from `kernel.now_ms()` which reads
    // through the injected `Clock`. A `FixedClock` therefore pins the
    // timestamp deterministically — load-bearing for replay.
    use std::sync::Arc;
    use std::time::{Duration, UNIX_EPOCH};
    let mut k = kernel();
    let fixed = super::clock::FixedClock(UNIX_EPOCH + Duration::from_millis(1_700_000_000_123));
    k.set_clock(Arc::new(fixed));
    k.record_action_stage("corr-clock", ActionStage::Requested, None);
    let proj = stages_proj(&mut k).unwrap();
    let entry = &proj["corr-clock"][0];
    assert_eq!(
        entry["at_ms"].as_u64().unwrap(),
        1_700_000_000_123,
        "at_ms must come from the kernel clock, not SystemTime::now"
    );
}

#[test]
fn record_action_failure_records_failed_stage_in_mirror() {
    // A sign-step failure (no
    // active account, malformed reply id, …) records a `Failed` stage into
    // `action_stages` *and* a terminal verdict into `action_results`. The
    // host listening on the stage seam sees the failure without subscribing
    // to action_results.
    let mut k = kernel();
    k.record_action_failure("corr-fail".to_string(), "no active account".to_string());
    let proj = stages_proj(&mut k).expect("the Failed stage surfaces in the mirror");
    let entry = &proj["corr-fail"][0];
    assert_eq!(entry["stage"], "failed");
    assert_eq!(entry["reason"], "no active account");
}

#[test]
fn failed_stage_survives_action_results_drain() {
    // The two surfaces are independent retention models:
    // - `action_results` drains on emit (per-tick edge)
    // - `action_stages` persists until ack (mirror)
    //
    // After `record_action_failure`, the FIRST tick emits both. The SECOND
    // tick omits `action_results` (drained) but still carries the `Failed`
    // stage entry in `action_stages` (mirror — until ack).
    let mut k = kernel();
    k.record_action_failure("corr-x".to_string(), "x".to_string());
    let _first = k.make_update_json_for_test(true);
    // Second tick — action_results is gone, action_stages persists.
    let snapshot_json = k.make_update_json_for_test(true);
    let snap: serde_json::Value = serde_json::from_str(&snapshot_json).unwrap();
    let projections = snap.get("projections").unwrap();
    assert!(
        projections.get("action_results").is_none(),
        "action_results drained after first emit"
    );
    assert!(
        projections.get("action_stages").is_some(),
        "action_stages still mirrors the failure"
    );

    // Ack closes the lifecycle.
    k.ack_action_stage("corr-x");
    let snap2: serde_json::Value =
        serde_json::from_str(&k.make_update_json_for_test(true)).unwrap();
    let projections2 = snap2.get("projections").unwrap();
    assert!(
        projections2.get("action_stages").is_none(),
        "ack drops the mirror entry"
    );
}

#[test]
fn lifecycle_four_stages_record_in_order() {
    // The publish lifecycle's canonical four stages — the consumer test.
    // Confirms the host can read the lifecycle out of one entry's history
    // in the order the kernel recorded it.
    let mut k = kernel();
    let cid = "corr-life";
    k.record_action_stage(cid, ActionStage::Requested, None);
    k.record_action_stage(cid, ActionStage::Publishing, None);
    k.record_action_stage(cid, ActionStage::Accepted, None);
    let proj = stages_proj(&mut k).unwrap();
    let history = proj[cid].as_array().unwrap();
    assert_eq!(history[0]["stage"], "requested");
    assert_eq!(history[1]["stage"], "publishing");
    assert_eq!(history[2]["stage"], "accepted");
}
