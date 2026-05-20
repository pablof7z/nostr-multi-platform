//! Host-extensible snapshot output — end-to-end proof for the
//! `nmp_app_register_snapshot_projection` seam.
//!
//! The bar (direction review #15): a host-registered projection must appear
//! in the JSON `make_update` emits — not merely in `SnapshotRegistry::run`
//! called in isolation. `make_update` is the JSON-emitting path the host
//! actually consumes, so the proof drives it and parses the result, mirroring
//! `t171_planner_error_projection_tests.rs` and `state_projection_tests.rs`.

use super::snapshot_registry::new_snapshot_projection_slot;
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// A projection registered before `make_update` must surface under
/// `projections["<key>"]` in the emitted snapshot JSON.
///
/// Pre-wiring: `KernelSnapshot` has no `projections` field → key absent.
/// Post-wiring: `make_update` runs `run_snapshot_projections()` → the
/// host's `{"count":42}` appears under `test.counter`.
#[test]
fn registered_projection_surfaces_through_make_update() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Bind a shared slot and register a projection — exactly what the actor
    // (`set_snapshot_projection_handle`) + FFI
    // (`nmp_app_register_snapshot_projection`) wiring does in production.
    let slot = new_snapshot_projection_slot();
    slot.lock()
        .unwrap()
        .register("test.counter", || serde_json::json!({ "count": 42 }));
    kernel.set_snapshot_projection_handle(slot);

    let snapshot_json = kernel.make_update(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    let count = parsed
        .get("projections")
        .and_then(|p| p.get("test.counter"))
        .and_then(|c| c.get("count"))
        .and_then(serde_json::Value::as_u64);
    assert_eq!(
        count,
        Some(42),
        "host projection must appear under projections[\"test.counter\"], got: {snapshot_json}"
    );
}

/// Multiple namespaces coexist — a marketplace and a todo app can each carry
/// their own snapshot namespace without colliding.
#[test]
fn multiple_projections_each_get_their_namespace() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    {
        let mut registry = slot.lock().unwrap();
        registry.register("market.listings", || serde_json::json!([{ "id": "a" }]));
        registry.register("todo.items", || serde_json::json!({ "open": 3 }));
    }
    kernel.set_snapshot_projection_handle(slot);

    let parsed: serde_json::Value =
        serde_json::from_str(&kernel.make_update(true)).expect("snapshot json");
    let projections = parsed.get("projections").expect("projections object");
    assert_eq!(
        projections.get("market.listings"),
        Some(&serde_json::json!([{ "id": "a" }]))
    );
    assert_eq!(
        projections.get("todo.items"),
        Some(&serde_json::json!({ "open": 3 }))
    );
}

/// Backwards compatibility: with no projection registered, the `projections`
/// key is `skip_serializing_if`'d entirely off the wire — a social-only shell
/// that predates this field decodes the snapshot unchanged.
#[test]
fn no_projection_omits_the_key_from_the_wire() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let parsed: serde_json::Value =
        serde_json::from_str(&kernel.make_update(true)).expect("snapshot json");
    assert!(
        parsed.get("projections").is_none(),
        "an empty projections map must not appear on the wire"
    );
}

/// A projection registered on the shared slot AFTER it was bound onto the
/// kernel still fires: the slot is `Arc`-shared, so a later registration
/// through any clone is visible to the next tick (the production FFI path
/// registers through the `NmpApp` clone after the actor already bound its
/// clone onto the kernel).
#[test]
fn projection_registered_after_binding_still_fires() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(Arc::clone(&slot));

    // First tick: nothing registered yet.
    let first: serde_json::Value =
        serde_json::from_str(&kernel.make_update(true)).expect("snapshot json");
    assert!(first.get("projections").is_none());

    // Register through the still-held `Arc` clone — as the FFI path does.
    slot.lock()
        .unwrap()
        .register("late.value", || serde_json::json!("present"));

    // Next tick picks it up.
    let second: serde_json::Value =
        serde_json::from_str(&kernel.make_update(true)).expect("snapshot json");
    assert_eq!(
        second
            .get("projections")
            .and_then(|p| p.get("late.value"))
            .and_then(serde_json::Value::as_str),
        Some("present"),
        "a projection registered after binding must fire on the next tick"
    );
}

/// `run_snapshot_projections` with no slot bound yields an empty map — D6:
/// a kernel constructed outside the actor never panics on the projection
/// path.
#[test]
fn unbound_slot_yields_empty_projections() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(kernel.run_snapshot_projections().is_empty());
}
