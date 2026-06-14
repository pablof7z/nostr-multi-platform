//! D5 — registration-count ceiling tests for [`SnapshotRegistry`].
//!
//! Proves the `MAX_SNAPSHOT_PROJECTIONS` / `MAX_TICK_OBSERVERS` bounds
//! (`snapshot_registry/bounds.rs`): a new key past the ceiling is a loud no-op,
//! while re-registering an existing key is always allowed. Counts are observed
//! through the public `run()` / `run_typed()` output (the registry's private
//! maps are not reachable from this sibling module — the public surface is the
//! contract these bounds protect).

use super::bounds::{MAX_SNAPSHOT_PROJECTIONS, MAX_TICK_OBSERVERS};
use super::{ChangeGate, SnapshotRegistry};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// D5: registering up to `MAX_SNAPSHOT_PROJECTIONS` keys surfaces all of them in
/// `run()`; the (MAX+1)-th new key is silently dropped (absent from output).
#[test]
fn generic_projection_registry_rejects_overflow() {
    let mut reg = SnapshotRegistry::new();

    for i in 0..MAX_SNAPSHOT_PROJECTIONS {
        reg.register(format!("test.key.{i}"), || serde_json::Value::Bool(true));
    }
    assert_eq!(
        reg.run().len(),
        MAX_SNAPSHOT_PROJECTIONS,
        "all keys up to the ceiling must surface in run()"
    );

    // One more NEW key — must be silently dropped.
    reg.register("test.key.overflow", || serde_json::Value::Bool(true));
    let output = reg.run();
    assert_eq!(
        output.len(),
        MAX_SNAPSHOT_PROJECTIONS,
        "D5 regression: registry grew past MAX_SNAPSHOT_PROJECTIONS after overflow"
    );
    assert!(
        !output.contains_key("test.key.overflow"),
        "D5 regression: overflowed projection key appeared in run() output"
    );
}

/// D5: re-registering an **existing** key at the ceiling replaces the closure
/// without growing the registry.
#[test]
fn generic_projection_registry_allows_re_registration_at_ceiling() {
    let mut reg = SnapshotRegistry::new();

    for i in 0..MAX_SNAPSHOT_PROJECTIONS {
        reg.register(format!("test.key.{i}"), || serde_json::Value::Null);
    }

    // Re-register an already-present key — must succeed, keep count at MAX, and
    // replace the old closure.
    reg.register("test.key.0", || serde_json::Value::Bool(true));
    let output = reg.run();
    assert_eq!(
        output.len(),
        MAX_SNAPSHOT_PROJECTIONS,
        "re-registration of an existing key must not grow the registry"
    );
    assert_eq!(
        output.get("test.key.0"),
        Some(&serde_json::Value::Bool(true)),
        "re-registered closure must replace the old one"
    );
}

/// D5: same ceiling for the **typed** projection registry.
#[test]
fn typed_projection_registry_rejects_overflow() {
    use crate::update_envelope::TypedProjectionData;
    let entry = || {
        Some(TypedProjectionData {
            key: "k".into(),
            schema_id: "k".into(),
            schema_version: 1,
            file_identifier: "TEST".into(),
            payload: vec![0u8],
            ..Default::default()
        })
    };

    let mut reg = SnapshotRegistry::new();
    for i in 0..MAX_SNAPSHOT_PROJECTIONS {
        reg.register_typed(format!("test.typed.{i}"), entry);
    }
    assert_eq!(reg.run_typed().len(), MAX_SNAPSHOT_PROJECTIONS);

    reg.register_typed("test.typed.overflow", entry);
    assert_eq!(
        reg.run_typed().len(),
        MAX_SNAPSHOT_PROJECTIONS,
        "D5 regression: typed registry grew past MAX_SNAPSHOT_PROJECTIONS"
    );
}

/// D5: same ceiling for the **gated** projection variant.
#[test]
fn gated_projection_registry_rejects_overflow() {
    let mut reg = SnapshotRegistry::new();
    let gate = Arc::new(AtomicU64::new(0));

    for i in 0..MAX_SNAPSHOT_PROJECTIONS {
        reg.register_gated(
            format!("test.gated.{i}"),
            Arc::clone(&gate) as Arc<dyn ChangeGate>,
            || serde_json::Value::Null,
        );
    }
    assert_eq!(reg.run().len(), MAX_SNAPSHOT_PROJECTIONS);

    reg.register_gated(
        "test.gated.overflow",
        Arc::clone(&gate) as Arc<dyn ChangeGate>,
        || serde_json::Value::Bool(true),
    );
    let output = reg.run();
    assert_eq!(
        output.len(),
        MAX_SNAPSHOT_PROJECTIONS,
        "D5 regression: gated registry grew past MAX_SNAPSHOT_PROJECTIONS"
    );
    assert!(!output.contains_key("test.gated.overflow"));
}

/// D5: tick-observer ceiling — the (MAX_TICK_OBSERVERS+1)-th registration is a
/// loud no-op. Observed by firing every registered observer once and counting
/// the side-effects (the observer list is not publicly enumerable).
#[test]
fn tick_observer_registry_rejects_overflow() {
    let mut reg = SnapshotRegistry::new();
    let fires = Arc::new(AtomicU64::new(0));

    for _ in 0..MAX_TICK_OBSERVERS {
        let f = Arc::clone(&fires);
        reg.register_tick_observer(move || {
            f.fetch_add(1, Ordering::Relaxed);
        });
    }

    // One more — must be dropped, so a single run still fires exactly
    // MAX_TICK_OBSERVERS observers (not MAX+1).
    let f = Arc::clone(&fires);
    reg.register_tick_observer(move || {
        f.fetch_add(1, Ordering::Relaxed);
    });

    reg.run_tick_observers();
    assert_eq!(
        fires.load(Ordering::Relaxed),
        MAX_TICK_OBSERVERS as u64,
        "D5 regression: tick-observer list grew past MAX_TICK_OBSERVERS"
    );
}
