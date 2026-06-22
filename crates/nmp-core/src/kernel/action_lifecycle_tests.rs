//! Tracker unit tests for the V5 `action_lifecycle` display projection.
//!
//! These exercise the `ActionLifecycleTracker` in isolation: stage
//! transitions, TTL drop, ordering, wire shape, cap behaviour. Kernel-level
//! contract tests (driving `Kernel::record_action_stage` etc.) live in the
//! sibling `action_lifecycle_kernel_tests.rs`.

use super::action_lifecycle::{
    ActionLifecycleTracker, LifecycleSnapshot, LifecycleStage, MAX_TRACKED_CORRELATIONS,
    RECENT_TERMINAL_TTL_MS,
};
use super::action_stages::{ActionStage, PENDING_STAGE_RETENTION_MS};

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
        LifecycleStage::Failed {
            reason,
            reason_code,
            ..
        } => {
            assert_eq!(reason, "no relays");
            // Un-coded path: a plain `record(Failed)` carries NO reason_code —
            // prose-only, the default for opaque text (#1735).
            assert_eq!(reason_code, &None);
        }
        other => panic!("expected Failed, got {:?}", other),
    }
}

/// `record_coded` attaches the curated `reason_code` (+ subject) to a `Failed`
/// display row while the prose `reason` is still carried (#1735). The snapshot
/// JSON serializes both — the shell localizes the code, falling back to prose.
#[test]
fn tracker_failed_coded_carries_reason_code_and_subject() {
    let mut t = ActionLifecycleTracker::new();
    t.record_coded(
        "corr-coded",
        ActionStage::Failed {
            reason: "no active account".to_string(),
        },
        Some("lifecycle_no_active_account"),
        Some("alice"),
        0,
    );

    let snap = t.snapshot(0);
    // Assert against the serialized JSON — the host-visible shape.
    let row = &snap["recent_terminal"][0];
    assert_eq!(row["stage"], "failed");
    assert_eq!(row["reason"], "no active account");
    assert_eq!(row["reason_code"], "lifecycle_no_active_account");
    assert_eq!(row["reason_subject"], "alice");

    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    match &payload.recent_terminal[0].stage {
        LifecycleStage::Failed {
            reason_code,
            reason_subject,
            ..
        } => {
            assert_eq!(reason_code.as_deref(), Some("lifecycle_no_active_account"));
            assert_eq!(reason_subject.as_deref(), Some("alice"));
        }
        other => panic!("expected Failed, got {:?}", other),
    }
}

/// `record_coded` with `reason_code = None` is equivalent to the prose-only
/// path: the snapshot omits the `reason_code`/`reason_subject` keys entirely
/// (the `skip_serializing_if` guard), so an un-coded reason never regresses a
/// host that branches on key presence.
#[test]
fn tracker_failed_uncoded_omits_reason_code_keys() {
    let mut t = ActionLifecycleTracker::new();
    t.record_coded(
        "corr-prose",
        ActionStage::Failed {
            reason: "boom".to_string(),
        },
        None,
        None,
        0,
    );
    let snap = t.snapshot(0);
    let row = &snap["recent_terminal"][0];
    assert_eq!(row["reason"], "boom");
    assert!(row.get("reason_code").is_none(), "un-coded reason must omit reason_code");
    assert!(
        row.get("reason_subject").is_none(),
        "un-coded reason must omit reason_subject"
    );
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

/// A non-terminal row is not dropped by the short terminal TTL. The longer
/// pending retention window owns eventual cleanup.
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

#[test]
fn tracker_non_terminal_drops_on_pending_ttl_expiry() {
    let mut t = ActionLifecycleTracker::new();
    t.record("corr-1", ActionStage::Publishing, 0);

    let snap = t.snapshot(PENDING_STAGE_RETENTION_MS);
    assert!(
        snap.is_null(),
        "snapshot is Null once pending retention expires"
    );
    assert_eq!(t.len(), 0, "pending entry was actually evicted");
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

