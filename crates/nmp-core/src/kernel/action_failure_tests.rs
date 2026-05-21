//! Broken-promise fix — `Kernel::record_action_failure` → `action_results`.
//!
//! A host that dispatches a `PublishNote` / `PublishProfile` through
//! `nmp_app_dispatch_action` receives a registry-minted `correlation_id` and
//! waits to see its outcome in the `action_results` snapshot projection. Every
//! terminal verdict for a *queued* publish reaches `action_results` via the
//! publish engine. But a failure in the *sign* step (no active account, a
//! malformed reply id, a local-key sign error, a remote-signer timeout /
//! rejection) aborts the publish *before* it ever reaches the engine — there
//! is no `PublishHandle`, no in-flight row.
//!
//! Before this fix those sign-step failures only set a toast; the host's
//! spinner keyed on the returned `correlation_id` would hang forever — a
//! broken promise (a correlation_id was returned but its outcome is never
//! observable). `Kernel::record_action_failure` closes that gap by pushing a
//! terminal `"failed"` verdict into the same per-tick `action_results` drain.
//!
//! These tests pin the *kernel-layer* contract — that `record_action_failure`
//! lands a `{correlation_id, status:"failed", error}` entry in the wire
//! snapshot. The engine-side push (`record_action_terminal_failure`) is
//! covered by `publish/engine/tests.rs`; the actor-loop wiring
//! (parked-remote-sign timeout / error) is covered there in lockstep.

use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// Read `projections.action_results` from a fresh wire snapshot. The key is
/// conditionally inserted (only when a terminal settled this tick), so absence
/// is reported here as `Null`.
fn action_results(kernel: &mut Kernel) -> serde_json::Value {
    let snapshot_json = kernel.make_update(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    parsed
        .get("projections")
        .and_then(|v| v.get("action_results"))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

#[test]
fn record_action_failure_surfaces_failed_terminal_in_action_results() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // No action recorded yet — the projection key is absent.
    assert!(
        action_results(&mut kernel).is_null(),
        "a kernel with no settled action has no action_results key"
    );

    kernel.record_action_failure(
        "corr-no-account".to_string(),
        "no active account".to_string(),
    );

    let results = action_results(&mut kernel);
    let arr = results
        .as_array()
        .expect("action_results must be a JSON array after a recorded failure");
    assert_eq!(arr.len(), 1, "exactly one terminal verdict this tick");
    let entry = &arr[0];
    assert_eq!(
        entry.get("correlation_id").and_then(|v| v.as_str()),
        Some("corr-no-account"),
        "the dispatch correlation_id is carried through so the host can match its spinner"
    );
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("failed"),
        "a sign-step failure reports the terminal `failed` status"
    );
    assert_eq!(
        entry.get("error").and_then(|v| v.as_str()),
        Some("no active account"),
        "the failure reason is carried verbatim for the host to display"
    );
}

#[test]
fn record_action_failure_is_drained_per_tick() {
    // `action_results` is a per-tick drain: the failure verdict appears once
    // and is consumed — a second snapshot tick (nothing new) omits the key.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.record_action_failure("corr-once".to_string(), "sign failed: rejected".to_string());

    assert!(
        action_results(&mut kernel).as_array().is_some(),
        "the first tick after a recorded failure carries the verdict"
    );
    assert!(
        action_results(&mut kernel).is_null(),
        "the verdict is drained — a second tick omits the action_results key"
    );
}

#[test]
fn multiple_action_failures_in_one_tick_all_survive() {
    // Two dispatched actions whose sign step fails between snapshot emits both
    // reach `action_results` — neither host spinner is stranded.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.record_action_failure("corr-a".to_string(), "no active account".to_string());
    kernel.record_action_failure(
        "corr-b".to_string(),
        "reply: malformed target event id".to_string(),
    );

    let results = action_results(&mut kernel);
    let arr = results
        .as_array()
        .expect("action_results must be a JSON array when failures were recorded");
    assert_eq!(arr.len(), 2, "both failures settle in the same tick");
    let mut ids: Vec<&str> = arr
        .iter()
        .filter_map(|item| item.get("correlation_id").and_then(|v| v.as_str()))
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec!["corr-a", "corr-b"],
        "both correlation_ids appear — the per-tick Vec accumulates before the drain"
    );
    for item in arr {
        assert_eq!(
            item.get("status").and_then(|v| v.as_str()),
            Some("failed"),
            "each recorded sign-step failure reports `failed`"
        );
    }
}
