//! D5 — registration-count ceiling tests for [`SnapshotRegistry`].
//!
//! Proves the `MAX_SNAPSHOT_PROJECTIONS` bound (`snapshot_registry/bounds.rs`):
//! a new key past the ceiling is a loud no-op, while re-registering an existing
//! key is always allowed. The generic (`serde_json::Value`) lane has been
//! removed; tests now cover only the typed ceiling.

use super::bounds::MAX_SNAPSHOT_PROJECTIONS;
use super::SnapshotRegistry;

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
