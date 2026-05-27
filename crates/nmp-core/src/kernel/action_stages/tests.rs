use super::*;

fn detail(s: &str) -> Option<serde_json::Value> {
    Some(serde_json::json!({ "note": s }))
}

/// Recording four stages produces a four-entry history in insertion
/// order, with timestamps preserved verbatim.
#[test]
fn record_appends_in_order() {
    let mut t = ActionStageTracker::new();
    let cid = "corr-1";
    t.record(cid, ActionStage::Requested, None, 1_000);
    t.record(cid, ActionStage::Publishing, detail("dispatch"), 1_010);
    t.record(cid, ActionStage::Accepted, None, 1_020);

    let history = t.history(cid).expect("history present");
    assert_eq!(history.len(), 3, "three stages were recorded");
    assert!(matches!(history[0].stage, ActionStage::Requested));
    assert!(matches!(history[1].stage, ActionStage::Publishing));
    assert!(matches!(history[2].stage, ActionStage::Accepted));
    assert_eq!(history[0].at_ms, 1_000);
    assert_eq!(history[2].at_ms, 1_020);
    assert!(
        history[1].detail.is_some(),
        "detail must be preserved verbatim"
    );
}

/// `Failed` carries an opaque reason string the host renders verbatim.
#[test]
fn failed_stage_carries_reason() {
    let mut t = ActionStageTracker::new();
    t.record(
        "corr-2",
        ActionStage::Failed {
            reason: "no relays".to_string(),
        },
        None,
        10,
    );
    let h = t.history("corr-2").unwrap();
    match &h[0].stage {
        ActionStage::Failed { reason } => assert_eq!(reason, "no relays"),
        other => panic!("expected Failed, got {:?}", other),
    }
}

/// `is_terminal` covers exactly `Accepted` and `Failed`.
#[test]
fn is_terminal_matches_only_terminal_variants() {
    assert!(!ActionStage::Requested.is_terminal());
    assert!(!ActionStage::AwaitingCapability.is_terminal());
    assert!(!ActionStage::Publishing.is_terminal());
    assert!(ActionStage::Accepted.is_terminal());
    assert!(ActionStage::Failed {
        reason: "x".to_string()
    }
    .is_terminal());
}

/// `ack` drops the entry; subsequent `history` returns `None`. This
/// is the load-bearing retention guarantee — stages persist *until*
/// ack, never on a TTL.
#[test]
fn ack_drops_entry() {
    let mut t = ActionStageTracker::new();
    let cid = "corr-ack";
    t.record(cid, ActionStage::Requested, None, 1);
    t.record(cid, ActionStage::Accepted, None, 2);
    assert!(t.history(cid).is_some());
    let removed = t.ack(cid);
    assert!(removed, "ack returns true when an entry was removed");
    assert!(t.history(cid).is_none(), "history is gone after ack");
    // Idempotent: a second ack is a silent no-op.
    let removed2 = t.ack(cid);
    assert!(!removed2);
}

/// `ack` of an unknown id is a silent no-op (D6).
#[test]
fn ack_unknown_is_noop() {
    let mut t = ActionStageTracker::new();
    let removed = t.ack("never-recorded");
    assert!(!removed);
    assert!(t.entries.is_empty());
}

/// THE LOAD-BEARING TEST: a Publishing-then-Accepted sequence is
/// preserved across many `snapshot()` calls — the snapshot is a
/// copy, not a drain, so the host can observe the same state
/// multiple ticks in a row until it acks.
#[test]
fn snapshot_is_a_copy_not_a_drain() {
    let mut t = ActionStageTracker::new();
    let cid = "corr-persist";
    t.record(cid, ActionStage::Requested, None, 1);
    t.record(cid, ActionStage::Accepted, None, 2);
    let snap_a = t.snapshot();
    let snap_b = t.snapshot();
    let snap_c = t.snapshot();
    assert_eq!(snap_a, snap_b);
    assert_eq!(snap_b, snap_c);
    // Entry is still there; only ack drops it.
    assert!(t.history(cid).is_some());
}

/// `snapshot()` returns `Null` when empty so the projection helper
/// can omit the key (parallels `action_results`'s convention).
#[test]
fn snapshot_is_null_when_empty() {
    let t = ActionStageTracker::new();
    assert!(t.snapshot().is_null());
}

/// Snapshot shape: each correlation_id maps to an array of stage
/// objects with `stage` (snake_cased), `at_ms`, optional `detail`.
/// This is the contract the host parses against.
#[test]
fn snapshot_shape_matches_host_expectations() {
    let mut t = ActionStageTracker::new();
    t.record("c1", ActionStage::Requested, None, 100);
    t.record(
        "c1",
        ActionStage::Publishing,
        Some(serde_json::json!({"relays": 3})),
        110,
    );

    let snap = t.snapshot();
    let obj = snap.as_object().expect("snapshot is a JSON object");
    let history = obj["c1"].as_array().expect("history is an array");
    assert_eq!(history.len(), 2);

    let first = &history[0];
    assert_eq!(first["stage"], "requested");
    assert_eq!(first["at_ms"], 100);
    // `detail` omitted (None) — `skip_serializing_if`.
    assert!(first.get("detail").is_none());

    let second = &history[1];
    assert_eq!(second["stage"], "publishing");
    assert_eq!(second["at_ms"], 110);
    assert_eq!(second["detail"], serde_json::json!({"relays": 3}));
}

/// `Failed` serialises with its inner `reason` field flattened
/// alongside the tag, matching serde's internally-tagged convention.
#[test]
fn failed_stage_serialises_with_reason() {
    let mut t = ActionStageTracker::new();
    t.record(
        "c-fail",
        ActionStage::Failed {
            reason: "no relays settled".to_string(),
        },
        None,
        7,
    );
    let snap = t.snapshot();
    let stage_obj = &snap["c-fail"][0];
    assert_eq!(stage_obj["stage"], "failed");
    assert_eq!(stage_obj["reason"], "no relays settled");
}

/// Per-correlation cap, non-terminal arrival: a non-terminal stage at
/// cap is silently dropped (diagnostic loss is safe — non-terminals
/// never drive UI cleanup). The history's existing entries survive.
#[test]
fn per_correlation_cap_drops_non_terminal_silently() {
    let mut t = ActionStageTracker::new();
    let cid = "c-cap";
    for i in 0..MAX_STAGES_PER_CORRELATION {
        t.record(cid, ActionStage::Publishing, None, i as u64);
    }
    assert_eq!(t.history(cid).unwrap().len(), MAX_STAGES_PER_CORRELATION);
    assert_eq!(t.per_correlation_cap_drops, 0);

    // A non-terminal arrival at cap is dropped silently.
    t.record(cid, ActionStage::Publishing, None, 999);
    assert_eq!(
        t.history(cid).unwrap().len(),
        MAX_STAGES_PER_CORRELATION,
        "history length is pinned at the cap"
    );
    assert_eq!(t.per_correlation_cap_drops, 1);
    assert_eq!(
        t.per_correlation_terminal_evictions, 0,
        "no terminal eviction occurred for a non-terminal drop"
    );
}

/// THE CONTRACT: at cap, a terminal stage MUST survive. The
/// `per_correlation_terminal_evictions` counter increments and the
/// oldest *non-terminal* entry is evicted to make room. A host that
/// keys its spinner cleanup on the terminal stage now sees it even
/// under a pathological retry storm.
#[test]
fn per_correlation_cap_evicts_non_terminal_to_seat_terminal() {
    let mut t = ActionStageTracker::new();
    let cid = "c-cap-term";
    for i in 0..MAX_STAGES_PER_CORRELATION {
        t.record(cid, ActionStage::Publishing, None, i as u64);
    }
    assert_eq!(t.history(cid).unwrap().len(), MAX_STAGES_PER_CORRELATION);

    // Arriving terminal: the oldest non-terminal is evicted; the
    // terminal IS recorded; size stays at the cap.
    t.record(cid, ActionStage::Accepted, None, 999);
    let history = t.history(cid).unwrap();
    assert_eq!(
        history.len(),
        MAX_STAGES_PER_CORRELATION,
        "size pins at the cap — one in, one out"
    );
    assert_eq!(t.per_correlation_terminal_evictions, 1);
    assert_eq!(
        t.per_correlation_cap_drops, 0,
        "no drop happened — the terminal was admitted, a non-terminal was evicted"
    );

    // THE LOAD-BEARING ASSERTION: the terminal is the LAST entry.
    let last = history.last().unwrap();
    assert!(
        matches!(last.stage, ActionStage::Accepted),
        "the terminal must survive at the tail of the history; got {:?}",
        last.stage
    );
    // The Failed-shape variant also survives — exercises the
    // `is_terminal` predicate on both arms.
    let mut t2 = ActionStageTracker::new();
    for i in 0..MAX_STAGES_PER_CORRELATION {
        t2.record("c2", ActionStage::Publishing, None, i as u64);
    }
    t2.record(
        "c2",
        ActionStage::Failed {
            reason: "fail".to_string(),
        },
        None,
        999,
    );
    let last2 = t2.history("c2").unwrap().last().unwrap();
    assert!(matches!(last2.stage, ActionStage::Failed { .. }));
}

/// Degenerate edge: a history full of terminals already (which a real
/// producer never builds — a correlation_id settles exactly once) still
/// admits a new terminal. The oldest terminal is evicted; the latest
/// one becomes the canonical tail. The "the latest terminal survives"
/// contract still holds.
#[test]
fn per_correlation_cap_terminal_at_cap_full_of_terminals() {
    let mut t = ActionStageTracker::new();
    let cid = "c-degen";
    for i in 0..MAX_STAGES_PER_CORRELATION {
        t.record(cid, ActionStage::Accepted, None, i as u64);
    }
    assert_eq!(t.history(cid).unwrap().len(), MAX_STAGES_PER_CORRELATION);

    t.record(
        cid,
        ActionStage::Failed {
            reason: "final".to_string(),
        },
        None,
        999,
    );
    let history = t.history(cid).unwrap();
    assert_eq!(history.len(), MAX_STAGES_PER_CORRELATION);
    // The latest terminal is the tail; the oldest was evicted.
    let last = history.last().unwrap();
    match &last.stage {
        ActionStage::Failed { reason } => assert_eq!(reason, "final"),
        other => panic!("expected Failed terminal at tail, got {other:?}"),
    }
    assert_eq!(
        history.first().unwrap().at_ms,
        1,
        "oldest entry was evicted"
    );
    assert_eq!(t.per_correlation_terminal_evictions, 1);
}

/// Global cap: the 1025th distinct correlation_id evicts the oldest
/// (front of the order vector) and increments the diagnostic.
/// Verifies the eviction is by first-record order, not by activity —
/// touching the second-oldest id after the cap does not bump it.
#[test]
fn global_cap_evicts_oldest_correlation() {
    let mut t = ActionStageTracker::new();
    for i in 0..MAX_TRACKED_CORRELATIONS {
        t.record(&format!("c-{i:04}"), ActionStage::Requested, None, i as u64);
    }
    // Touch c-0001 — should NOT change its eviction order (the
    // tracker uses first-record, not last-touch, as the eviction key).
    t.record("c-0001", ActionStage::Publishing, None, 9_999);
    assert_eq!(t.len(), MAX_TRACKED_CORRELATIONS);

    // The 1025th distinct id triggers eviction of c-0000.
    t.record("c-new", ActionStage::Requested, None, 10_000);
    assert_eq!(t.len(), MAX_TRACKED_CORRELATIONS, "size is pinned at cap");
    assert!(
        t.history("c-0000").is_none(),
        "the oldest correlation_id is evicted"
    );
    assert!(
        t.history("c-0001").is_some(),
        "the second-oldest survives — eviction is by first-record, not last-touch"
    );
    assert_eq!(t.global_cap_evictions, 1);

    // The order vector front is now c-0001.
    let order = t.order_snapshot();
    assert_eq!(order.first().map(String::as_str), Some("c-0001"));
    assert_eq!(order.last().map(String::as_str), Some("c-new"));
}

/// Re-recording an existing correlation_id after ack treats it as a
/// new entry — the eviction order picks up the *current* moment, so
/// the post-ack lifecycle is fresh. This is the contract a host
/// retrying the same action handle relies on.
#[test]
fn record_after_ack_starts_fresh() {
    let mut t = ActionStageTracker::new();
    let cid = "c-retry";
    t.record(cid, ActionStage::Requested, None, 1);
    t.record(cid, ActionStage::Accepted, None, 2);
    assert!(t.ack(cid));

    t.record(cid, ActionStage::Requested, None, 3);
    let h = t.history(cid).unwrap();
    assert_eq!(
        h.len(),
        1,
        "post-ack record is the first of a fresh history"
    );
    assert_eq!(h[0].at_ms, 3);
}
