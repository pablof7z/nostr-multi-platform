//! Unit tests for the `ActionLedger`'s DERIVED `action_lifecycle` view.
//!
//! These port the prior `ActionLifecycleTracker` unit tests onto the one
//! ledger, proving the derived `lifecycle_snapshot` output is byte-identical to
//! the deleted parallel tracker: same latest-stage collapse, same first-record
//! ordering, same terminal vs pending TTL, same curated `reason_code` /
//! `reason_subject` (#1735). Kernel-level contract tests (driving
//! `Kernel::record_action_stage` etc.) live in `action_lifecycle_kernel_tests.rs`.

use crate::kernel::action_ledger::{
    ActionLedger, LifecycleSnapshot, LifecycleStage, RECENT_TERMINAL_TTL_MS,
};
use crate::kernel::action_stages::{
    ActionStage, MAX_STAGES_PER_CORRELATION, MAX_TRACKED_CORRELATIONS, PENDING_STAGE_RETENTION_MS,
};

/// Helper: record a stage with no detail / no curated code.
pub(super) fn rec(ledger: &mut ActionLedger, cid: &str, stage: ActionStage, at_ms: u64) {
    ledger.record(cid, stage, None, at_ms);
}

// ─── Derived lifecycle view tests ────────────────────────────────────────

/// A non-terminal stage surfaces the correlation_id in `in_flight`.
#[test]
fn requested_lands_in_in_flight() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-1", ActionStage::Requested, 1_000);

    let snap = l.lifecycle_snapshot(1_000);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(payload.recent_terminal.len(), 0);
    assert_eq!(payload.in_flight[0].correlation_id, "corr-1");
    assert_eq!(payload.in_flight[0].stage, LifecycleStage::Requested);
}

/// Transitioning through Publishing keeps the id in `in_flight` at the latest
/// stage (latest-stage-wins collapse over the history).
#[test]
fn publishing_replaces_requested_in_in_flight() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-1", ActionStage::Requested, 1_000);
    rec(&mut l, "corr-1", ActionStage::Publishing, 1_100);

    let snap = l.lifecycle_snapshot(1_100);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(payload.in_flight[0].stage, LifecycleStage::Publishing);
}

/// `Accepted` moves the id from `in_flight` to `recent_terminal`.
#[test]
fn accepted_moves_to_recent_terminal() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-1", ActionStage::Requested, 1_000);
    rec(&mut l, "corr-1", ActionStage::Accepted, 1_500);

    let snap = l.lifecycle_snapshot(1_500);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 0);
    assert_eq!(payload.recent_terminal.len(), 1);
    assert_eq!(payload.recent_terminal[0].correlation_id, "corr-1");
    assert_eq!(payload.recent_terminal[0].stage, LifecycleStage::Accepted);
}

/// `Failed` lands in `recent_terminal` with the reason verbatim; the un-coded
/// path carries NO reason_code (prose-only, the #1735 default).
#[test]
fn failed_lands_in_recent_terminal_with_reason() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-fail", ActionStage::Requested, 0);
    rec(
        &mut l,
        "corr-fail",
        ActionStage::Failed {
            reason: "no relays".to_string(),
        },
        10,
    );

    let snap = l.lifecycle_snapshot(10);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.recent_terminal.len(), 1);
    match &payload.recent_terminal[0].stage {
        LifecycleStage::Failed {
            reason,
            reason_code,
            ..
        } => {
            assert_eq!(reason, "no relays");
            assert_eq!(reason_code, &None);
        }
        other => panic!("expected Failed, got {:?}", other),
    }
}

/// `record_coded` attaches the curated `reason_code` (+ subject) to the derived
/// `Failed` row while the substrate history keeps prose only (#1735).
#[test]
fn failed_coded_carries_reason_code_and_subject() {
    let mut l = ActionLedger::new();
    l.record_coded(
        "corr-coded",
        ActionStage::Failed {
            reason: "no active account".to_string(),
        },
        None,
        Some("lifecycle_no_active_account"),
        Some("alice"),
        0,
    );

    let snap = l.lifecycle_snapshot(0);
    let row = &snap["recent_terminal"][0];
    assert_eq!(row["stage"], "failed");
    assert_eq!(row["reason"], "no active account");
    assert_eq!(row["reason_code"], "lifecycle_no_active_account");
    assert_eq!(row["reason_subject"], "alice");

    // The substrate `action_stages` history keeps ONLY the prose reason — the
    // curated code never bleeds into the history (it rides the lifecycle view).
    let stages = l.stages_snapshot(0);
    let entry = &stages["corr-coded"][0];
    assert_eq!(entry["stage"], "failed");
    assert_eq!(entry["reason"], "no active account");
    assert!(
        entry.get("reason_code").is_none(),
        "substrate history must not carry the curated reason_code"
    );

    let payload: LifecycleSnapshot = serde_json::from_value(l.lifecycle_snapshot(0)).unwrap();
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

/// `record_coded` with `reason_code = None` omits the keys (skip_serializing_if).
#[test]
fn failed_uncoded_omits_reason_code_keys() {
    let mut l = ActionLedger::new();
    l.record_coded(
        "corr-prose",
        ActionStage::Failed {
            reason: "boom".to_string(),
        },
        None,
        None,
        None,
        0,
    );
    let snap = l.lifecycle_snapshot(0);
    let row = &snap["recent_terminal"][0];
    assert_eq!(row["reason"], "boom");
    assert!(row.get("reason_code").is_none());
    assert!(row.get("reason_subject").is_none());
}

/// Terminal rows drop on TTL expiry at the `>=` boundary.
#[test]
fn terminal_drops_on_ttl_expiry() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-1", ActionStage::Accepted, 1_000);

    let inside = l.lifecycle_snapshot(1_000 + RECENT_TERMINAL_TTL_MS - 1);
    let payload: LifecycleSnapshot = serde_json::from_value(inside).unwrap();
    assert_eq!(payload.recent_terminal.len(), 1);

    let at = l.lifecycle_snapshot(1_000 + RECENT_TERMINAL_TTL_MS);
    assert!(at.is_null(), "snapshot is Null once arrays empty post-TTL");
    assert_eq!(l.len(), 0, "entry was actually evicted");
}

/// A non-terminal row survives the short terminal TTL.
#[test]
fn non_terminal_survives_ttl_window() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-1", ActionStage::Publishing, 0);

    let snap = l.lifecycle_snapshot(RECENT_TERMINAL_TTL_MS * 10);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(payload.in_flight[0].stage, LifecycleStage::Publishing);
}

#[test]
fn non_terminal_drops_on_pending_ttl_expiry() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-1", ActionStage::Publishing, 0);

    let snap = l.lifecycle_snapshot(PENDING_STAGE_RETENTION_MS);
    assert!(snap.is_null());
    assert_eq!(l.len(), 0);
}

/// Empty ledger derives a `Null` lifecycle snapshot.
#[test]
fn empty_snapshot_is_null() {
    let mut l = ActionLedger::new();
    assert!(l.lifecycle_snapshot(0).is_null());
}

/// Multiple ids surface in first-record order; touching an older id does not
/// reorder it.
#[test]
fn ordering_is_first_record_stable() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-a", ActionStage::Requested, 100);
    rec(&mut l, "corr-b", ActionStage::Requested, 200);
    rec(&mut l, "corr-a", ActionStage::Publishing, 250);
    rec(&mut l, "corr-c", ActionStage::Requested, 300);

    let snap = l.lifecycle_snapshot(300);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    let ids: Vec<&str> = payload
        .in_flight
        .iter()
        .map(|e| e.correlation_id.as_str())
        .collect();
    assert_eq!(ids, vec!["corr-a", "corr-b", "corr-c"]);
}

/// An in-flight action coexists with a recent terminal.
#[test]
fn in_flight_and_recent_terminal_coexist() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-done", ActionStage::Accepted, 100);
    rec(&mut l, "corr-busy", ActionStage::Publishing, 110);

    let snap = l.lifecycle_snapshot(110);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(payload.in_flight[0].correlation_id, "corr-busy");
    assert_eq!(payload.recent_terminal.len(), 1);
    assert_eq!(payload.recent_terminal[0].correlation_id, "corr-done");
}

/// Wire shape flattens `stage` + `reason` + `correlation_id`.
#[test]
fn wire_shape_flattens_stage_and_reason() {
    let mut l = ActionLedger::new();
    rec(
        &mut l,
        "corr-fail",
        ActionStage::Failed {
            reason: "boom".to_string(),
        },
        42,
    );

    let snap = l.lifecycle_snapshot(42);
    let obj = snap.as_object().expect("snapshot is JSON object");
    let recent = obj["recent_terminal"].as_array().unwrap();
    let entry = &recent[0];
    assert_eq!(entry["stage"], "failed");
    assert_eq!(entry["reason"], "boom");
    assert_eq!(entry["correlation_id"], "corr-fail");
}

/// Global cardinality cap evicts the oldest id; the derived view and the
/// sidecar shrink in lock-step with the substrate history.
#[test]
fn global_cap_evicts_oldest_correlation() {
    let mut l = ActionLedger::new();
    for i in 0..MAX_TRACKED_CORRELATIONS {
        rec(
            &mut l,
            &format!("c-{i:04}"),
            ActionStage::Requested,
            i as u64,
        );
    }
    assert_eq!(l.len(), MAX_TRACKED_CORRELATIONS);

    rec(&mut l, "c-new", ActionStage::Requested, 9_999);
    assert_eq!(l.len(), MAX_TRACKED_CORRELATIONS, "size pins at cap");
    assert!(
        l.history("c-0000").is_none(),
        "oldest correlation_id evicted"
    );
    assert!(l.history("c-new").is_some());
}

/// Re-recording an existing id does not consume a fresh cap slot.
#[test]
fn re_recording_existing_id_does_not_consume_cap() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-1", ActionStage::Requested, 0);
    rec(&mut l, "corr-1", ActionStage::Publishing, 1);
    rec(&mut l, "corr-1", ActionStage::Accepted, 2);
    assert_eq!(l.len(), 1);
}

/// `AwaitingCapability` is non-terminal — stays in `in_flight`.
#[test]
fn awaiting_capability_is_in_flight() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-bunker", ActionStage::AwaitingCapability, 0);

    let snap = l.lifecycle_snapshot(0);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(
        payload.in_flight[0].stage,
        LifecycleStage::AwaitingCapability
    );
}

/// Ack early-dismisses the id from BOTH derived views (one ledger row).
#[test]
fn ack_drops_from_both_views() {
    let mut l = ActionLedger::new();
    rec(&mut l, "corr-ack", ActionStage::Accepted, 0);
    assert!(!l.lifecycle_snapshot(0).is_null());
    assert!(!l.stages_snapshot(0).is_null());

    assert!(l.ack("corr-ack"));
    assert!(l.lifecycle_snapshot(0).is_null());
    assert!(l.stages_snapshot(0).is_null());
    // Idempotent: a second ack is a silent no-op.
    assert!(!l.ack("corr-ack"));
}

/// THE BYTE-IDENTITY EDGE (codex S11 review): at the per-correlation history
/// cap, the bounded `action_stages` history silently DROPS a non-terminal
/// record — but the derived `action_lifecycle` view MUST still advance to that
/// latest stage and re-anchor its TTL, exactly as the deleted
/// `ActionLifecycleTracker` did (it overwrote its single latest slot on every
/// record, independent of the history cap). The latest-lifecycle facet is the
/// authoritative source for the view, so it does not regress here.
#[test]
fn lifecycle_advances_past_history_per_correlation_cap() {
    let mut l = ActionLedger::new();
    let cid = "c-cap";
    // Fill the history to its per-correlation cap with non-terminal stages.
    // The LAST one recorded is `AwaitingCapability` at a known timestamp.
    for i in 0..MAX_STAGES_PER_CORRELATION {
        let stage = if i + 1 == MAX_STAGES_PER_CORRELATION {
            ActionStage::AwaitingCapability
        } else {
            ActionStage::Publishing
        };
        l.record(cid, stage, None, i as u64);
    }
    // The 65th non-terminal record: the history DROPS it (cap), but the
    // lifecycle latest slot must advance to `Publishing` at the new timestamp.
    let cap_at = MAX_STAGES_PER_CORRELATION as u64; // 64
    l.record(cid, ActionStage::Publishing, None, cap_at);

    // History's latest entry is the pre-cap `AwaitingCapability` (the 65th was
    // dropped) — proves the history really hit the cap.
    let hist = l.history(cid).unwrap();
    assert_eq!(hist.len(), MAX_STAGES_PER_CORRELATION);
    assert!(matches!(
        hist.last().unwrap().stage,
        ActionStage::AwaitingCapability
    ));

    // The DERIVED lifecycle view shows the post-cap `Publishing` — the latest
    // slot advanced even though the history dropped the diagnostic row.
    let snap = l.lifecycle_snapshot(cap_at);
    let payload: LifecycleSnapshot = serde_json::from_value(snap).unwrap();
    assert_eq!(payload.in_flight.len(), 1);
    assert_eq!(payload.in_flight[0].stage, LifecycleStage::Publishing);

    // The TTL anchor advanced too: the slot survives just before its pending
    // TTL relative to the NEW timestamp, and a stale anchor would have expired.
    let still_live = l.lifecycle_snapshot(cap_at + PENDING_STAGE_RETENTION_MS - 1);
    let payload2: LifecycleSnapshot = serde_json::from_value(still_live).unwrap();
    assert_eq!(
        payload2.in_flight.len(),
        1,
        "TTL anchor re-anchored to the latest record"
    );
}

/// The curated sidecar is reconciled when its id leaves the history (cap or
/// ack) so it can never outgrow the bounded log (D8).
#[test]
fn coded_sidecar_pruned_when_id_evicted() {
    let mut l = ActionLedger::new();
    l.record_coded(
        "corr-coded",
        ActionStage::Failed {
            reason: "x".to_string(),
        },
        None,
        Some("code_x"),
        None,
        0,
    );
    // TTL expiry drops the row; the next derive sees no curated code leak.
    let _ = l.lifecycle_snapshot(RECENT_TERMINAL_TTL_MS);
    assert_eq!(l.len(), 0);

    // A NEW record under the same id must NOT inherit the stale code.
    rec(
        &mut l,
        "corr-coded",
        ActionStage::Failed {
            reason: "y".to_string(),
        },
        RECENT_TERMINAL_TTL_MS,
    );
    let snap = l.lifecycle_snapshot(RECENT_TERMINAL_TTL_MS);
    let row = &snap["recent_terminal"][0];
    assert_eq!(row["reason"], "y");
    assert!(
        row.get("reason_code").is_none(),
        "a re-recorded id must not inherit a stale curated reason_code"
    );
}
