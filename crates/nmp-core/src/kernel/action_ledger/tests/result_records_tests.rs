//! Unit tests for the `ActionLedger`'s DERIVED `action_results` drain
//! (S11 slice 2, #1758).
//!
//! Proves that `record_terminal` + `take_terminal_results` produce the same
//! per-tick drain semantics as the deleted `pending_terminals`-sourced path:
//! same `ok → published` mapping, same `error` / `result` / `event_id` fields,
//! same producer-order accumulation, and same `reason_code` threading into the
//! lifecycle view (not the `action_results` row prose).

use crate::kernel::action_ledger::ActionLedger;
use crate::kernel::action_stages::ActionStage;

// ─── Derived action_results drain tests (S11 slice 2, #1758) ──────────────

/// `record_terminal` appends one `action_results` row carrying the already-
/// mapped wire status, the verbatim error, and (when present) the result /
/// event_id. `take_terminal_results` drains it once.
#[test]
fn record_terminal_drains_one_row_then_empties() {
    let mut l = ActionLedger::new();
    l.record_terminal(
        "corr-pub",
        ActionStage::Accepted,
        "published",
        None,
        None,
        Some("event-abc".to_string()),
        None,
        None,
        1_000,
    );

    let rows = l.take_terminal_results();
    assert_eq!(rows.len(), 1, "exactly one row per terminal");
    let row = &rows[0];
    assert_eq!(row["correlation_id"], "corr-pub");
    assert_eq!(row["status"], "published", "ok → published mapping is resolved at record time");
    assert!(row["error"].is_null(), "success carries a null error key");
    assert!(row.get("result").is_none(), "no result_json → no result key");
    assert_eq!(row["event_id"], "event-abc");

    // Pure drain — the next call is empty (the terminal appears exactly once).
    assert!(
        l.take_terminal_results().is_empty(),
        "the terminal is drained — a second take yields nothing"
    );
}

/// A failed terminal carries the verbatim error and `status: "failed"`; the
/// stage ALSO lands in the derived lifecycle view in the same write.
#[test]
fn record_terminal_failed_row_and_lifecycle_mirror() {
    let mut l = ActionLedger::new();
    l.record_terminal(
        "corr-fail",
        ActionStage::Failed {
            reason: "no relays".to_string(),
        },
        "failed",
        Some("no relays".to_string()),
        None,
        Some("ev-1".to_string()),
        None,
        None,
        2_000,
    );

    let rows = l.take_terminal_results();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"], "failed");
    assert_eq!(rows[0]["error"], "no relays");
    assert_eq!(rows[0]["event_id"], "ev-1");

    // The same write mirrored the Failed stage into the lifecycle view.
    let snap = l.lifecycle_snapshot(2_000);
    assert_eq!(snap["recent_terminal"][0]["correlation_id"], "corr-fail");
    assert_eq!(snap["recent_terminal"][0]["stage"], "failed");
    assert_eq!(snap["recent_terminal"][0]["reason"], "no relays");
}

/// The curated `reason_code` / `reason_subject` (#1735) thread into the derived
/// lifecycle view (Failed only), not the `action_results` row prose.
#[test]
fn record_terminal_threads_reason_code_into_lifecycle() {
    let mut l = ActionLedger::new();
    l.record_terminal(
        "corr-coded",
        ActionStage::Failed {
            reason: "refused".to_string(),
        },
        "failed",
        Some("refused".to_string()),
        None,
        None,
        Some("LIFECYCLE_X"),
        Some("subj"),
        3_000,
    );

    // The action_results row keeps prose only (no reason_code key there).
    let rows = l.take_terminal_results();
    assert!(rows[0].get("reason_code").is_none());

    // The lifecycle view carries the curated code + subject.
    let snap = l.lifecycle_snapshot(3_000);
    let row = &snap["recent_terminal"][0];
    assert_eq!(row["reason_code"], "LIFECYCLE_X");
    assert_eq!(row["reason_subject"], "subj");
}

/// Two terminals recorded between drains both survive a single drain in
/// producer order — the per-tick buffer accumulates (no spinner stranded).
#[test]
fn record_terminal_accumulates_until_drained() {
    let mut l = ActionLedger::new();
    l.record_terminal("corr-a", ActionStage::Accepted, "published", None, None, None, None, None, 1);
    l.record_terminal(
        "corr-b",
        ActionStage::Failed {
            reason: "x".to_string(),
        },
        "failed",
        Some("x".to_string()),
        None,
        None,
        None,
        None,
        2,
    );

    let rows = l.take_terminal_results();
    let ids: Vec<&str> = rows
        .iter()
        .map(|r| r["correlation_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["corr-a", "corr-b"], "producer order preserved");
}

/// `result_json` is forwarded into `result` as a parsed JSON object (ADR-0043
/// Decision 4); a non-JSON body forwards as a raw string.
#[test]
fn record_terminal_forwards_result_json() {
    let mut l = ActionLedger::new();
    l.record_terminal(
        "corr-blob",
        ActionStage::Accepted,
        "published",
        None,
        Some(r#"{"sha256":"abc"}"#.to_string()),
        None,
        None,
        None,
        1,
    );
    l.record_terminal(
        "corr-raw",
        ActionStage::Accepted,
        "published",
        None,
        Some("not json".to_string()),
        None,
        None,
        None,
        2,
    );

    let rows = l.take_terminal_results();
    assert!(rows[0]["result"].is_object(), "JSON body parses to an object");
    assert_eq!(rows[0]["result"]["sha256"], "abc");
    assert_eq!(rows[1]["result"], "not json", "non-JSON body forwards as a raw string");
}
