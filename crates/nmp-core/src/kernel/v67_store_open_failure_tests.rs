//! V-67 regression tests — `store_open_failure` must be projected through the
//! KernelUpdate JSON envelope so the host can observe a degraded store state.
//!
//! The failure mode being fixed: when `build_event_store` was given a path but
//! the LMDB open failed, it silently fell back to `MemEventStore` with no
//! diagnostic emitted. The host reported healthy; all locally-stored events
//! were lost on next launch.
//!
//! The test seam `set_store_open_failure_for_test` injects the failure state
//! that `build_event_store` would set on a real LMDB open error, so we can
//! verify the projection without requiring the `lmdb-backend` feature or a
//! real filesystem failure. The pattern mirrors T171 (`last_planner_error`).

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// When `store_open_failure` is set (LMDB open failed at kernel init), the
/// string must appear in the JSON KernelUpdate the FFI emits.
///
/// Pre-fix: `make_update` never read `self.store_open_failure` → the key was
/// absent from the snapshot → host could not observe the degradation → FAILS.
/// Post-fix: `make_update` projects it → key carries the failure string → PASSES.
#[test]
fn v67_store_open_failure_is_projected_through_ffi_snapshot() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Simulate the state `build_event_store` sets when the LMDB open fails.
    kernel.set_store_open_failure_for_test(
        "LMDB open failed: No such file or directory (os error 2)",
    );

    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    let surfaced = parsed
        .get("store_open_failure")
        .and_then(serde_json::Value::as_str);

    assert_eq!(
        surfaced,
        Some("LMDB open failed: No such file or directory (os error 2)"),
        "V-67 (D6): a store-open failure must be projected through the \
         KernelUpdate/FFI JSON envelope so the host can surface it to the user; \
         got: {:?}",
        parsed.get("store_open_failure")
    );
}

/// Steady state: with no open failure recorded the `store_open_failure` key
/// must be absent from the wire (omitted by `skip_serializing_if`), never
/// present as `null` or a stale string. Guards against the projection emitting
/// noise on the healthy path.
#[test]
fn v67_no_store_open_failure_key_is_absent_from_snapshot() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    // The field must be completely absent (skip_serializing_if = Option::is_none),
    // not present as JSON null — the wire stays byte-for-byte identical to
    // pre-V-67 snapshots when there is no failure.
    assert!(
        !parsed.as_object().map(|o| o.contains_key("store_open_failure")).unwrap_or(false),
        "V-67: with no store-open failure the `store_open_failure` key must be \
         absent from the snapshot (skip_serializing_if); got: {:?}",
        parsed.get("store_open_failure")
    );
}
