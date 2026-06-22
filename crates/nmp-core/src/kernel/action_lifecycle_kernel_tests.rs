//! Kernel contract tests for the V5 `action_lifecycle` display projection.
//!
//! These drive the actual `Kernel::record_action_stage` /
//! `record_action_failure` / `record_action_success` callers and pull the
//! projection out of the emitted snapshot JSON — the boundary the iOS shell
//! consumes. Split from `action_lifecycle_tests.rs` to keep each file under
//! the 500-LOC hard cap (AGENTS.md §file-size). Tracker unit tests live in
//! `action_lifecycle_tests.rs`.

use super::action_stages::ActionStage;
use super::Kernel;

// ─── Kernel contract tests — projection in the snapshot JSON ─────────────

fn kernel() -> Kernel {
    Kernel::new(64)
}

/// Pull the `action_lifecycle` projection out of the kernel's snapshot
/// JSON. Returns `None` when the projection is absent (steady state).
fn lifecycle_proj(kernel: &mut Kernel) -> Option<serde_json::Value> {
    let snapshot_json = kernel.make_update_json_for_test(true);
    let snap: serde_json::Value = serde_json::from_str(&snapshot_json).expect("update JSON parses");
    snap.get("projections")
        .and_then(|p| p.get("action_lifecycle"))
        .cloned()
}

#[test]
fn empty_kernel_omits_lifecycle_projection() {
    let mut k = kernel();
    // No records → the projection key must be absent. Steady-state hot
    // path must not carry empty payloads.
    assert!(lifecycle_proj(&mut k).is_none());
}

#[test]
fn requested_stage_surfaces_in_in_flight() {
    let mut k = kernel();
    k.record_action_stage("corr-a", ActionStage::Requested, None);

    let proj = lifecycle_proj(&mut k).expect("projection emitted after record");
    let in_flight = proj["in_flight"].as_array().expect("in_flight is array");
    assert_eq!(in_flight.len(), 1);
    assert_eq!(in_flight[0]["correlation_id"], "corr-a");
    assert_eq!(in_flight[0]["stage"], "requested");

    let recent = proj["recent_terminal"]
        .as_array()
        .expect("recent_terminal is array");
    assert!(recent.is_empty(), "no terminal yet");
}

#[test]
fn accepted_stage_moves_entry_to_recent_terminal() {
    let mut k = kernel();
    k.record_action_stage("corr-a", ActionStage::Requested, None);
    k.record_action_stage("corr-a", ActionStage::Accepted, None);

    let proj = lifecycle_proj(&mut k).expect("projection emitted after terminal");
    let in_flight = proj["in_flight"].as_array().unwrap();
    assert!(in_flight.is_empty(), "entry no longer in flight");

    let recent = proj["recent_terminal"].as_array().unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["correlation_id"], "corr-a");
    assert_eq!(recent[0]["stage"], "accepted");
}

#[test]
fn ack_early_dismisses_lifecycle_before_ttl() {
    let mut k = kernel();
    k.record_action_stage("corr-dismiss", ActionStage::Requested, None);
    k.record_action_stage("corr-dismiss", ActionStage::Accepted, None);

    let before = lifecycle_proj(&mut k).expect("terminal lifecycle emitted");
    assert_eq!(
        before["recent_terminal"][0]["correlation_id"],
        "corr-dismiss"
    );

    k.ack_action_stage("corr-dismiss");
    assert!(
        lifecycle_proj(&mut k).is_none(),
        "ack is an early-dismiss cleanup path for action_lifecycle, not the only retention mechanism"
    );
}

#[test]
fn failed_stage_carries_reason_in_recent_terminal() {
    let mut k = kernel();
    k.record_action_stage(
        "corr-fail",
        ActionStage::Failed {
            reason: "no relays".to_string(),
        },
        None,
    );

    let proj = lifecycle_proj(&mut k).expect("projection emitted");
    let recent = proj["recent_terminal"].as_array().unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["stage"], "failed");
    assert_eq!(recent[0]["reason"], "no relays");
    assert_eq!(recent[0]["correlation_id"], "corr-fail");
}

#[test]
fn record_action_failure_lifts_into_lifecycle() {
    // `record_action_failure` is the sign-step-error path (a dispatched
    // action whose publish never reached the engine). It must mirror into
    // the lifecycle projection the same way an engine-driven terminal
    // does — otherwise a host listening only on `action_lifecycle` would
    // miss the failure.
    let mut k = kernel();
    k.record_action_failure("corr-sign".to_string(), "bad sig".to_string());

    let proj = lifecycle_proj(&mut k).expect("projection emitted");
    let recent = proj["recent_terminal"].as_array().unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["correlation_id"], "corr-sign");
    assert_eq!(recent[0]["stage"], "failed");
    assert_eq!(recent[0]["reason"], "bad sig");
}

#[test]
fn record_action_success_lifts_into_lifecycle() {
    // `record_action_success` is the off-band success path (NIP-47 NWC
    // pay_invoice → kind:23195 ack). It must mirror into the lifecycle
    // projection identically to `record_action_failure`.
    let mut k = kernel();
    k.record_action_success("corr-ok".to_string(), None);

    let proj = lifecycle_proj(&mut k).expect("projection emitted");
    let recent = proj["recent_terminal"].as_array().unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["correlation_id"], "corr-ok");
    assert_eq!(recent[0]["stage"], "accepted");
}

#[test]
fn multiple_correlations_coexist_with_stable_order() {
    let mut k = kernel();
    k.record_action_stage("corr-a", ActionStage::Requested, None);
    k.record_action_stage("corr-b", ActionStage::Publishing, None);
    k.record_action_stage("corr-c", ActionStage::Accepted, None);

    let proj = lifecycle_proj(&mut k).expect("projection emitted");
    let in_flight = proj["in_flight"].as_array().unwrap();
    let recent = proj["recent_terminal"].as_array().unwrap();

    assert_eq!(in_flight.len(), 2, "corr-a + corr-b in flight");
    assert_eq!(recent.len(), 1, "corr-c terminal");
    // first-record order preserved within each array
    assert_eq!(in_flight[0]["correlation_id"], "corr-a");
    assert_eq!(in_flight[1]["correlation_id"], "corr-b");
    assert_eq!(recent[0]["correlation_id"], "corr-c");
}

#[test]
fn lifecycle_and_stages_share_terminal_in_same_tick() {
    // The two projections are additive — `action_stages` carries the full
    // history for diagnostic consumers, `action_lifecycle` the display
    // collapse. A terminal recorded once must appear in both surfaces on
    // the SAME snapshot tick (single `record_action_stage` call).
    let mut k = kernel();
    k.record_action_stage("corr-both", ActionStage::Accepted, None);

    let snapshot_json = k.make_update_json_for_test(true);
    let snap: serde_json::Value = serde_json::from_str(&snapshot_json).expect("update JSON parses");
    let projections = snap.get("projections").unwrap();

    let stages = projections.get("action_stages").expect("stages emitted");
    let lifecycle = projections
        .get("action_lifecycle")
        .expect("lifecycle emitted");

    // action_stages: history array under correlation_id key.
    let stage_history = stages["corr-both"].as_array().unwrap();
    assert_eq!(stage_history.len(), 1);
    assert_eq!(stage_history[0]["stage"], "accepted");

    // action_lifecycle: entry in recent_terminal.
    let recent = lifecycle["recent_terminal"].as_array().unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["correlation_id"], "corr-both");
    assert_eq!(recent[0]["stage"], "accepted");
}
