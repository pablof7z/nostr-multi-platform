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

/// With no *host* projection registered, the `projections` map carries only
/// the kernel-owned built-in projections — and no host namespace.
///
/// D0: `make_update` always inserts the publish cluster (`publish_queue` /
/// `publish_outbox` / `relay_edit_rows`) and the identity pair (`accounts` /
/// `active_account`) — all kernel-owned domain state, not host registrations —
/// so the map is never empty and `skip_serializing_if` no longer drops it. A
/// host that registers nothing simply contributes no extra keys: the social
/// shell still sees only the five built-ins it expects.
#[test]
fn no_host_projection_leaves_only_the_builtin_projections() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let parsed: serde_json::Value =
        serde_json::from_str(&kernel.make_update(true)).expect("snapshot json");
    let projections = parsed
        .get("projections")
        .expect("the built-in projections keep the projections map on the wire");
    let map = projections
        .as_object()
        .expect("projections must serialize as a JSON object");
    let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "accounts",
            "active_account",
            "publish_outbox",
            "publish_queue",
            "relay_edit_rows"
        ],
        "with no host projection the map carries only the kernel-owned built-ins"
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

    // First tick: no host projection registered yet — the map carries only
    // the kernel-owned built-in publish cluster, never the `late.value` key.
    let first: serde_json::Value =
        serde_json::from_str(&kernel.make_update(true)).expect("snapshot json");
    assert!(
        first
            .get("projections")
            .and_then(|p| p.get("late.value"))
            .is_none(),
        "a host projection must not appear before it is registered"
    );

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

/// D6 — a host projection closure that panics is contained: its key is
/// omitted (the same shape as an unregistered namespace), every other
/// projection in the same tick still produces its value, and the actor
/// thread is never unwound.
///
/// Without the per-closure `catch_unwind` guard, a single buggy host plugin
/// would panic *inside* `make_update` on the actor thread — the actor's
/// outer `catch_unwind` then catches a terminal `Panic` frame and the
/// kernel is permanently dead. A snapshot projection MUST never be able to
/// kill the kernel.
#[test]
fn panicking_projection_is_contained_and_others_survive() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    {
        let mut registry = slot.lock().unwrap();
        // A well-behaved projection registered alongside the bad one.
        registry.register("good.value", || serde_json::json!({ "ok": true }));
        // A buggy host plugin: panics every time it is polled.
        registry.register("bad.value", || -> serde_json::Value {
            panic!("buggy host projection");
        });
    }
    kernel.set_snapshot_projection_handle(slot);

    // First tick: the panic must not propagate out of `make_update`.
    let first: serde_json::Value =
        serde_json::from_str(&kernel.make_update(true)).expect("snapshot json survives a panic");
    let projections = first
        .get("projections")
        .expect("the surviving projection must still produce a projections object");
    assert_eq!(
        projections.get("good.value"),
        Some(&serde_json::json!({ "ok": true })),
        "a panicking sibling must not poison the other projections in the same tick",
    );
    assert!(
        projections.get("bad.value").is_none(),
        "a panicking projection's key must be omitted, not surfaced as garbage: {first}",
    );

    // Second tick: the kernel is still alive and still emits a valid
    // snapshot — the panic did not unwind the actor / kernel.
    let second: serde_json::Value = serde_json::from_str(&kernel.make_update(true))
        .expect("the kernel must survive a panicking projection and keep ticking");
    assert_eq!(
        second
            .get("projections")
            .and_then(|p| p.get("good.value")),
        Some(&serde_json::json!({ "ok": true })),
        "the good projection must keep firing on every subsequent tick",
    );
}

/// D0 — first internal consumer of the snapshot-projection seam: the `"wallet"`
/// projection.
///
/// NIP-47 NWC is an app noun, so wallet state was removed as a typed
/// `KernelSnapshot.wallet_status` field and is now surfaced through a
/// host-registered `"wallet"` projection — the same seam a marketplace or todo
/// app uses. This test wires the projection exactly as `nmp_app_new` does (a
/// closure over the shared `WalletStatusSlot`) and drives it through the real
/// `make_update` JSON path, asserting the connect → disconnect lifecycle:
///
/// - no wallet connected → `projections["wallet"]` is JSON `null`;
/// - wallet connected → `projections["wallet"]` carries the serialized status;
/// - wallet disconnected → `projections["wallet"]` clears back to `null`,
///   never a stale `ready` card.
#[cfg(feature = "wallet")]
#[test]
fn wallet_projection_appears_and_clears_through_make_update() {
    use crate::actor::{new_wallet_status_slot, WalletStatus};

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // The shared wallet-status slot — in production the actor's `WalletRuntime`
    // is the sole writer (D4) and `nmp_app_new` captures a clone in the
    // `"wallet"` projection closure. Here the test plays both roles.
    let wallet_status = new_wallet_status_slot();
    let projection_slot = new_snapshot_projection_slot();
    {
        // Register the SAME closure `nmp_app_new` installs: serialize the slot,
        // contributing `null` when no wallet is connected (D6: a poisoned
        // mutex also collapses to `null`).
        let wallet_status = wallet_status.clone();
        projection_slot.lock().unwrap().register("wallet", move || {
            match wallet_status.lock() {
                Ok(slot) => slot
                    .as_ref()
                    .map(|status| {
                        serde_json::to_value(status).unwrap_or(serde_json::Value::Null)
                    })
                    .unwrap_or(serde_json::Value::Null),
                Err(_) => serde_json::Value::Null,
            }
        });
    }
    kernel.set_snapshot_projection_handle(projection_slot);

    // No wallet connected → projections["wallet"] is JSON null.
    let before: serde_json::Value =
        serde_json::from_str(&kernel.make_update(true)).expect("snapshot json");
    assert!(
        before
            .get("projections")
            .and_then(|p| p.get("wallet"))
            .map(serde_json::Value::is_null)
            .unwrap_or(true),
        "with no wallet connected projections[\"wallet\"] must be null, got: {before}"
    );

    // Connect a wallet — write to the shared slot exactly as the actor's
    // `sync_wallet_status` does.
    *wallet_status.lock().unwrap() = Some(WalletStatus {
        status: "ready".to_string(),
        relay_url: "wss://wallet.example/".to_string(),
        wallet_npub: "npub1walletexample".to_string(),
        balance_msats: Some(21_000),
    });
    let connected: serde_json::Value =
        serde_json::from_str(&kernel.make_update(true)).expect("snapshot json");
    let wallet = connected
        .get("projections")
        .and_then(|p| p.get("wallet"))
        .expect("projections[\"wallet\"] must be present once a wallet connects");
    assert_eq!(
        wallet.get("status").and_then(serde_json::Value::as_str),
        Some("ready"),
        "a connected wallet must project status=ready",
    );
    assert_eq!(
        wallet.get("relay_url").and_then(serde_json::Value::as_str),
        Some("wss://wallet.example/"),
        "the wallet relay URL must be projected",
    );
    assert_eq!(
        wallet.get("balance_msats").and_then(serde_json::Value::as_u64),
        Some(21_000),
        "the wallet balance must be projected when known",
    );

    // Disconnect → the projection clears back to null, not a stale `ready`.
    *wallet_status.lock().unwrap() = None;
    let disconnected: serde_json::Value =
        serde_json::from_str(&kernel.make_update(true)).expect("snapshot json");
    assert!(
        disconnected
            .get("projections")
            .and_then(|p| p.get("wallet"))
            .map(serde_json::Value::is_null)
            .unwrap_or(true),
        "after disconnect projections[\"wallet\"] must clear to null, got: {disconnected}"
    );
}
