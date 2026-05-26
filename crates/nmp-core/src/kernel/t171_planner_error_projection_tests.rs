//! T171 RED — `last_planner_error` must be projected through KernelUpdate/FFI.
//!
//! T140-FF added `SubscriptionLifecycle::last_planner_error()` so genuine
//! structural planner errors are RECORDED (the `drain_tick` `Err(e)` arm).
//! But that recorded error was never projected through `KernelUpdate` / the
//! JSON FFI envelope, so the host still saw "empty frames" with no surfaced
//! error — D6 violated across the C-ABI.
//!
//! `PlannerError` variants are presently defensive: `compile_with_context`
//! always returns `Ok`, so no variant is constructed on a real compiler path.
//! The test seam `set_planner_error_for_test` injects the recorded-error
//! state the `Err(e)` arm would set, proving the projection is wired so any
//! future genuine construction path surfaces automatically.
//!
//! This test MUST FAIL before the projection lands (the JSON key is absent)
//! and MUST PASS after.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// A forced `last_planner_error` must appear in the JSON KernelUpdate the FFI
/// emits (`make_update` is the JSON-emitting path the host consumes).
///
/// Pre-fix: `make_update` never reads `lifecycle.last_planner_error()` → the
/// `last_planner_error` key is absent / null → FAILS.
/// Post-fix: `make_update` projects it → key carries the forced string →
/// PASSES.
#[test]
fn t171_forced_planner_error_is_observable_through_ffi_projection() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Inject the recorded-error state the genuine `drain_tick` Err(e) arm
    // would set (PlannerError variants are defensive-only today).
    kernel
        .lifecycle_mut()
        .set_planner_error_for_test("invalid shape: until < since");

    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    let surfaced = parsed
        .get("last_planner_error")
        .and_then(serde_json::Value::as_str);

    assert_eq!(
        surfaced,
        Some("invalid shape: until < since"),
        "T171 (D6): a recorded planner error must be projected through the \
         KernelUpdate/FFI JSON envelope so the host can observe it instead of \
         seeing silent empty frames; got: {:?}",
        parsed.get("last_planner_error")
    );
}

/// Steady state: with NO planner error recorded the projected field must be
/// `null` (absent value), never a stale or fabricated string. Guards against
/// the projection emitting noise on the happy path.
#[test]
fn t171_no_planner_error_projects_null() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    assert!(
        parsed
            .get("last_planner_error")
            .map(serde_json::Value::is_null)
            .unwrap_or(true),
        "T171: with no recorded planner error the projected field must be \
         null, not a fabricated string; got: {:?}",
        parsed.get("last_planner_error")
    );
}
