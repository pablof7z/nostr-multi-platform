//! D5 — registration-count ceiling tests for [`SnapshotRegistry`].
//!
//! Proves the `MAX_SNAPSHOT_PROJECTIONS` / `MAX_TICK_OBSERVERS` bounds
//! (`snapshot_registry/bounds.rs`): a new key past the ceiling is a loud no-op,
//! while re-registering an existing key is always allowed. The generic
//! (`serde_json::Value`) lane has been removed; tests now cover only the typed
//! and tick-observer ceilings.

use super::SnapshotRegistry;
use super::bounds::{MAX_SNAPSHOT_PROJECTIONS, MAX_TICK_OBSERVERS};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// D5: re-registering an **existing** typed key at the ceiling replaces the
/// closure without growing the registry.
#[test]
fn typed_projection_registry_allows_re_registration_at_ceiling() {
    use crate::update_envelope::TypedProjectionData;
    let entry_a = || {
        Some(TypedProjectionData {
            key: "k".into(),
            schema_id: "schema-a".into(),
            schema_version: 1,
            file_identifier: "TEST".into(),
            payload: vec![0u8],
            ..Default::default()
        })
    };
    let entry_b = || {
        Some(TypedProjectionData {
            key: "k".into(),
            schema_id: "schema-b".into(),
            schema_version: 2,
            file_identifier: "TEST".into(),
            payload: vec![1u8],
            ..Default::default()
        })
    };

    let mut reg = SnapshotRegistry::new();
    for i in 0..MAX_SNAPSHOT_PROJECTIONS {
        reg.register_typed(format!("test.typed.{i}"), entry_a);
    }
    // Re-register key 0 — must succeed, keep count at MAX, replace the closure.
    reg.register_typed("test.typed.0", entry_b);
    let typed = reg.run_typed();
    assert_eq!(
        typed.len(),
        MAX_SNAPSHOT_PROJECTIONS,
        "re-registration of an existing key must not grow the registry"
    );
    let key0 = typed
        .iter()
        .find(|t| t.key == "k" && t.schema_id == "schema-b");
    assert!(
        key0.is_some(),
        "re-registered closure must replace the old one"
    );
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

#[test]
fn keyed_tick_observer_replaces_and_removes_without_growing() {
    let mut reg = SnapshotRegistry::new();
    let marker = Arc::new(AtomicU64::new(0));

    let first = Arc::clone(&marker);
    reg.replace_tick_observer("marmot.expiry", move || {
        first.store(1, Ordering::Relaxed);
    });
    let second = Arc::clone(&marker);
    reg.replace_tick_observer("marmot.expiry", move || {
        second.store(2, Ordering::Relaxed);
    });

    reg.run_tick_observers();
    assert_eq!(marker.load(Ordering::Relaxed), 2);
    assert!(reg.remove_tick_observer("marmot.expiry"));

    marker.store(0, Ordering::Relaxed);
    reg.run_tick_observers();
    assert_eq!(marker.load(Ordering::Relaxed), 0);
    assert!(!reg.remove_tick_observer("marmot.expiry"));
}
