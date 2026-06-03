//! D13 sign-and-return — `Kernel::record_signed_event_return` → the
//! `signed_events` snapshot projection.
//!
//! The sign-and-return seam (`ActorCommand::SignEventForReturn`) parks a
//! signed event (or an error) under a `correlation_id` for the host to read
//! out-of-band — it NEVER publishes. These tests pin the projection's shape and
//! its drain-once semantics, mirroring `action_failure_tests` for the
//! `action_results` projection: the same `Null -> omit key` + per-tick-drain
//! contract the Swift `signEventForReturn` continuation bridge depends on.

use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// Read `projections.signed_events` from a fresh wire snapshot. The key is
/// conditionally inserted (only when a result settled this tick), so absence is
/// reported here as `Null`.
fn signed_events(kernel: &mut Kernel) -> serde_json::Value {
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    parsed
        .get("projections")
        .and_then(|v| v.get("signed_events"))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

#[test]
fn steady_state_omits_the_signed_events_key() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(
        signed_events(&mut kernel).is_null(),
        "a kernel with no sign-and-return result has no signed_events key"
    );
}

#[test]
fn ok_result_surfaces_signed_json_keyed_by_correlation_id() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let signed_json = r#"{"id":"ab","pubkey":"cd","created_at":1,"kind":24242,"tags":[],"content":"Upload image","sig":"ef"}"#;
    kernel.record_signed_event_return("corr-ok", Ok(signed_json.to_string()));

    let projection = signed_events(&mut kernel);
    let entry = projection
        .get("corr-ok")
        .expect("the result is keyed by its correlation_id");
    assert_eq!(
        entry.get("ok").and_then(serde_json::Value::as_bool),
        Some(true),
        "a successful sign reports ok=true"
    );
    assert_eq!(
        entry.get("signed_json").and_then(serde_json::Value::as_str),
        Some(signed_json),
        "the signed event JSON is carried verbatim for the host to attach to a transport"
    );
    assert!(
        entry.get("error").is_none(),
        "a successful sign carries no error field"
    );
}

#[test]
fn err_result_surfaces_error_keyed_by_correlation_id() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.record_signed_event_return(
        "corr-err",
        Err("no active account — sign in first".to_string()),
    );

    let projection = signed_events(&mut kernel);
    let entry = projection
        .get("corr-err")
        .expect("the failure is keyed by its correlation_id");
    assert_eq!(
        entry.get("ok").and_then(serde_json::Value::as_bool),
        Some(false),
        "a failed sign reports ok=false"
    );
    assert_eq!(
        entry.get("error").and_then(serde_json::Value::as_str),
        Some("no active account — sign in first"),
        "the failure reason is carried verbatim for the host's continuation to throw"
    );
    assert!(
        entry.get("signed_json").is_none(),
        "a failed sign carries no signed_json field"
    );
}

#[test]
fn signed_events_is_drained_per_tick() {
    // Drain-once, mirroring `action_results`: the result appears on the first
    // tick and is consumed — a second tick (nothing new) omits the key. The
    // host reads each id exactly once.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.record_signed_event_return("corr-once", Ok("{}".to_string()));

    assert!(
        signed_events(&mut kernel).get("corr-once").is_some(),
        "the first tick after a recorded result carries it"
    );
    assert!(
        signed_events(&mut kernel).is_null(),
        "a second tick with nothing new omits the signed_events key (drain-once)"
    );
}

#[test]
fn multiple_results_in_one_tick_all_surface() {
    // Two results recorded before a single emit both appear — the host can
    // resolve every waiting continuation from one tick, none hangs.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.record_signed_event_return("corr-a", Ok("{\"a\":1}".to_string()));
    kernel.record_signed_event_return("corr-b", Err("rejected".to_string()));

    let projection = signed_events(&mut kernel);
    assert_eq!(
        projection.get("corr-a").and_then(|v| v.get("ok")),
        Some(&serde_json::Value::Bool(true)),
        "the success result surfaces"
    );
    assert_eq!(
        projection.get("corr-b").and_then(|v| v.get("ok")),
        Some(&serde_json::Value::Bool(false)),
        "the failure result surfaces in the same tick"
    );
}
