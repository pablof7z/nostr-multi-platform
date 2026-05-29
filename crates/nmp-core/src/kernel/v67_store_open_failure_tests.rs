//! V-67 regression tests — `store_open_failure` must be projected through the
//! KernelUpdate JSON envelope so the host can observe a degraded store state.
//!
//! The failure mode being fixed: when `build_event_store` was given a path but
//! the LMDB open failed, it silently fell back to `MemEventStore` with no
//! diagnostic emitted. The host reported healthy; all locally-stored events
//! were lost on next launch.
//!
//! Test structure:
//!
//! 1. Seam tests (always compiled) — verify the projection plumbing by
//!    injecting the failure state via `set_store_open_failure_for_test`. This
//!    mirrors T171 (`last_planner_error`), where the seam is the only way to
//!    reach a defensive path. These tests prove the snapshot JSON carries the
//!    field when set, and omits it when not set.
//!
//! 2. Feature-gated integration test (`cfg(feature = "lmdb-backend")`) —
//!    exercises the actual `build_event_store` failure branch by constructing a
//!    kernel with a storage path that points at an existing regular *file*
//!    (not a directory). `open_impl` calls `std::fs::create_dir_all` first,
//!    which fails with `ENOTDIR` on a file path, so the `Err` arm fires and
//!    `store_open_failure` becomes `Some`. This is the test that proves the fix
//!    exists — the seam tests would pass even if the `match` arm were reverted.

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

// ─── Feature-gated integration tests (lmdb-backend) ──────────────────────────
//
// These tests exercise the actual `build_event_store` failure branch end-to-end.
// They require `--features lmdb-backend` to compile; the path-as-file trick
// triggers the LMDB open error deterministically without any I/O timing races.

/// V-67 integration test: constructing a kernel with a storage path that points
/// at an existing regular *file* (not a directory) causes `build_event_store` to
/// receive `Err` from `LmdbEventStore::open`. The kernel must still construct
/// (app can run), but `store_open_failure` must be `Some` on the first snapshot.
///
/// `open_impl` calls `std::fs::create_dir_all(path)` before calling `open_env`.
/// Passing a regular file makes `create_dir_all` fail with `ENOTDIR`,
/// which propagates as `StoreError::Io(...)`, triggering the `Err(e)` arm in
/// `build_event_store` and setting `kernel.store_open_failure = Some(reason)`.
#[cfg(feature = "lmdb-backend")]
#[test]
fn v67_kernel_with_file_path_sets_store_open_failure() {
    use std::io::Write;

    // Create a temp dir and a regular file inside it — LMDB cannot treat a
    // file as an environment directory.
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("not_a_dir.db");
    std::fs::File::create(&file_path)
        .expect("create file")
        .write_all(b"not an lmdb env")
        .expect("write sentinel");

    let path_str = file_path.to_str().expect("valid utf-8");
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, Some(path_str));

    // The failure reason must surface in the first snapshot.
    // (Direct field access is private; the snapshot JSON is the observable
    // surface the host consumes — testing via JSON matches production usage.)
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    assert!(
        parsed
            .get("store_open_failure")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "V-67: the store_open_failure reason must appear in the KernelUpdate snapshot; \
         got: {:?}",
        parsed.get("store_open_failure")
    );
}

// Note: a test asserting that `store_open_failure` is absent when LMDB opens
// successfully is not included here because the test environment does not have
// enough address space to map the 32 GB LMDB env (open_env returns "No space
// left on device"). The seam test `v67_no_store_open_failure_key_is_absent_from_snapshot`
// above covers that assertion path without requiring a real LMDB open.
