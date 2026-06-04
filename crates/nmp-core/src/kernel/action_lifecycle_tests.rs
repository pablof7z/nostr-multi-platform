//! Tests for the V5 `action_lifecycle` display projection — both the
//! standalone tracker and its kernel-level contract surface.
//!
//! Tracker unit tests (the `tracker_*` block at the top) exercise the
//! `ActionLifecycleTracker` in isolation: stage transitions, TTL drop,
//! ordering, wire shape, cap behaviour. The kernel-level contract tests
//! (the `kernel_*` block below) drive the actual `Kernel::record_action_stage`
//! / `record_action_failure` / `record_action_success` callers and pull
//! the projection out of the emitted snapshot JSON — the boundary the iOS
//! shell consumes.

use super::action_lifecycle::{
    ActionLifecycleTracker, LifecycleSnapshot, LifecycleStage, MAX_TRACKED_CORRELATIONS,
    RECENT_TERMINAL_TTL_MS,
};
use super::action_stages::ActionStage;
use super::Kernel;

// ─── ActionLifecycleTracker unit tests ───────────────────────────────────

/// Recording a non-terminal stage surfaces the correlation_id in
/// `in_flight` on the next snapshot.
#[test]
fn tracker_requested_lands_in_in_flight() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-1", ActionStage::Requested, 1_000);

    let snap = t.snapshot(1_000);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(payload.recent_terminal.len(), 0);
    assert_eq!(payload.in_flight[0].correlation_id, "corr-1");
    assert_eq!(payload.in_flight[0].stage, LifecycleStage::Requested);
}

/// Transitioning a correlation_id through Publishing keeps it in
/// `in_flight` and shows the latest stage.
#[test]
fn tracker_publishing_replaces_requested_in_in_flight() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-1", ActionStage::Requested, 1_000);
    t.record("corr-1", ActionStage::Publishing, 1_100);

    let snap = t.snapshot(1_100);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(payload.in_flight[0].stage, LifecycleStage::Publishing);
}

/// Recording `Accepted` moves the correlation_id from `in_flight` to
/// `recent_terminal` on the next snapshot.
#[test]
fn tracker_accepted_moves_to_recent_terminal() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-1", ActionStage::Requested, 1_000);
    t.record("corr-1", ActionStage::Accepted, 1_500);

    let snap = t.snapshot(1_500);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 0);
    assert_eq!(payload.recent_terminal.len(), 1);
    assert_eq!(payload.recent_terminal[0].correlation_id, "corr-1");
    assert_eq!(payload.recent_terminal[0].stage, LifecycleStage::Accepted);
}

/// `Failed` lands in `recent_terminal` and surfaces the reason verbatim.
#[test]
fn tracker_failed_lands_in_recent_terminal_with_reason() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-fail", ActionStage::Requested, 0);
    t.record(
        "corr-fail",
        ActionStage::Failed {
            reason: "no relays".to_string(),
        },
        10,
    );

    let snap = t.snapshot(10);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.recent_terminal.len(), 1);
    match &payload.recent_terminal[0].stage {
        LifecycleStage::Failed { reason } => assert_eq!(reason, "no relays"),
        other => panic!("expected Failed, got {:?}", other),
    }
}

/// Terminal rows drop on TTL expiry. Snapshotting at exactly
/// `latest_at_ms + RECENT_TERMINAL_TTL_MS` drops the row (>= boundary).
#[test]
fn tracker_terminal_drops_on_ttl_expiry() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-1", ActionStage::Accepted, 1_000);

    // Within TTL — still present.
    let snap_inside = t.snapshot(1_000 + RECENT_TERMINAL_TTL_MS - 1);
    let payload: LifecycleSnapshot = serde_json::from_value(snap_inside).unwrap();
    assert_eq!(payload.recent_terminal.len(), 1);

    // At TTL — dropped.
    let snap_at = t.snapshot(1_000 + RECENT_TERMINAL_TTL_MS);
    assert!(
        snap_at.is_null(),
        "snapshot is Null once both arrays are empty post-TTL"
    );
    assert_eq!(t.len(), 0, "entry was actually evicted, not just hidden");
}

/// A non-terminal row is *not* dropped by TTL — only terminals are.
/// A long-running publish that never settles stays in `in_flight`
/// until a terminal stage transitions it (or the global cap evicts).
#[test]
fn tracker_non_terminal_survives_ttl_window() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-1", ActionStage::Publishing, 0);

    // Well past TTL — still in in_flight.
    let snap = t.snapshot(RECENT_TERMINAL_TTL_MS * 10);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(payload.in_flight[0].stage, LifecycleStage::Publishing);
}

/// Steady state — no records — produces a `Null` snapshot so the
/// projection key is absent in the snapshot map (zero wire bytes).
#[test]
fn tracker_empty_snapshot_is_null() {
    let mut t = ActionLifecycleTracker::new();
    let snap = t.snapshot(0);
    assert!(snap.is_null());
}

/// Multiple correlation_ids surface in first-record order so the host
/// renders a stable spinner list across ticks. A fresh dispatch lands
/// at the bottom; a subsequent stage transition on an older id does
/// not reorder.
#[test]
fn tracker_ordering_is_first_record_stable() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-a", ActionStage::Requested, 100);
    t.record("corr-b", ActionStage::Requested, 200);
    // Touch the older id — must not bump it to the bottom.
    t.record("corr-a", ActionStage::Publishing, 250);
    t.record("corr-c", ActionStage::Requested, 300);

    let snap = t.snapshot(300);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    let ids: Vec<&str> = payload
        .in_flight
        .iter()
        .map(|e| e.correlation_id.as_str())
        .collect();
    assert_eq!(ids, vec!["corr-a", "corr-b", "corr-c"]);
}

/// Both arrays may carry rows in the same snapshot — an in-flight
/// action coexists with a recent terminal until the TTL expires the
/// latter.
#[test]
fn tracker_in_flight_and_recent_terminal_coexist() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-done", ActionStage::Accepted, 100);
    t.record("corr-busy", ActionStage::Publishing, 110);

    let snap = t.snapshot(110);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(payload.in_flight[0].correlation_id, "corr-busy");
    assert_eq!(payload.recent_terminal.len(), 1);
    assert_eq!(payload.recent_terminal[0].correlation_id, "corr-done");
}

/// The wire shape is `{stage: "<snake>", correlation_id: "...", [reason]}`
/// — `Failed`'s `reason` is flattened alongside `stage` and
/// `correlation_id` (matches the `ActionStage` serde convention).
#[test]
fn tracker_wire_shape_flattens_stage_and_reason() {
    let mut t = ActionLifecycleTracker::new();
    t.record(
        "corr-fail",
        ActionStage::Failed {
            reason: "boom".to_string(),
        },
        42,
    );

    let snap = t.snapshot(42);
    // Top-level: in_flight, recent_terminal.
    let obj = snap.as_object().expect("snapshot is JSON object");
    let recent = obj["recent_terminal"].as_array().unwrap();
    let entry = &recent[0];
    assert_eq!(entry["stage"], "failed");
    assert_eq!(entry["reason"], "boom");
    assert_eq!(entry["correlation_id"], "corr-fail");
}

/// Global cardinality cap evicts the oldest correlation_id when the
/// 1025th distinct id is recorded. Mirrors
/// `ActionStageTracker::record`'s overflow semantics.
#[test]
fn tracker_global_cap_evicts_oldest_correlation() {
    let mut t = ActionLifecycleTracker::new();
    for i in 0..MAX_TRACKED_CORRELATIONS {
        t.record(&format!("c-{i:04}"), ActionStage::Requested, i as u64);
    }
    assert_eq!(t.len(), MAX_TRACKED_CORRELATIONS);

    t.record("c-new", ActionStage::Requested, 9_999);
    assert_eq!(t.len(), MAX_TRACKED_CORRELATIONS, "size pins at cap");
    assert!(!t.contains("c-0000"), "oldest correlation_id evicted");
    assert!(t.contains("c-new"));
    assert_eq!(t.global_cap_evictions, 1);
}

/// Re-recording an existing correlation_id does not double-count the
/// global cap. Only the *first* record for a cid takes a slot.
#[test]
fn tracker_re_recording_existing_id_does_not_consume_cap() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-1", ActionStage::Requested, 0);
    t.record("corr-1", ActionStage::Publishing, 1);
    t.record("corr-1", ActionStage::Accepted, 2);
    assert_eq!(t.len(), 1);
    assert_eq!(t.global_cap_evictions, 0);
}

/// `AwaitingCapability` is non-terminal — bunker handshakes / MLS
/// pending signers stay in `in_flight` until they settle.
#[test]
fn tracker_awaiting_capability_is_in_flight() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-bunker", ActionStage::AwaitingCapability, 0);

    let snap = t.snapshot(0);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(
        payload.in_flight[0].stage,
        LifecycleStage::AwaitingCapability
    );
}

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
