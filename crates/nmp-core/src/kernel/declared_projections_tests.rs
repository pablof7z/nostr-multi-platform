//! ADR-0053 — host-declared projection subscriptions: end-to-end gating proofs.
//!
//! These drive `make_update` (the path the host actually consumes) and assert
//! WHICH Tier-2 built-in projection keys appear in the emitted snapshot under
//! the host-declared consumed-projection set:
//!
//! - empty declared set ⇒ every Tier-2 built-in is present (no narrowing);
//! - non-empty declared set ⇒ only declared keys present, undeclared omitted;
//! - the gate applies to BOTH the generic JSON map and the typed sidecar (the
//!   ADR-0037 divergence-safety invariant extended to the gate);
//! - the drain-on-emit keys (`action_results` …) still work when declared;
//! - `relay_diagnostics` — the headline waste — is omitted unless declared.

use crate::kernel::snapshot_registry::new_snapshot_projection_slot;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// Drive one tick and return the parsed `projections` JSON object.
fn projections_json(kernel: &mut Kernel) -> serde_json::Map<String, serde_json::Value> {
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    parsed
        .get("projections")
        .and_then(|p| p.as_object())
        .cloned()
        .unwrap_or_default()
}

/// A kernel with a bound (empty-declared) registry slot — the production wiring.
fn kernel_with_slot() -> (
    Kernel,
    crate::kernel::snapshot_registry::SnapshotProjectionSlot,
) {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(slot.clone());
    (kernel, slot)
}

/// Empty declared set = NO narrowing: every Tier-2 built-in is emitted, exactly
/// as before ADR-0053. This is the "host expressed no opinion" semantic and the
/// guarantee that the kernel's own Rust consumers / test helpers keep working
/// with zero declaration.
#[test]
fn empty_declared_set_emits_all_builtins() {
    let (mut kernel, _slot) = kernel_with_slot();
    let projections = projections_json(&mut kernel);

    // The unconditional Tier-2 built-ins must all be present (the drain-on-emit
    // four are absent in steady state — that is their own convention, unrelated
    // to the declared-set gate).
    for key in [
        "publish_queue",
        "publish_outbox",
        "outbox_summary",
        "configured_relays",
        "relay_role_options",
        "settings_hub",
        "accounts",
        "active_account",
        "profile",
        "relay_diagnostics",
        "mention_profiles",
        "claimed_profiles",
        "claimed_events",
        "resolved_profiles",
    ] {
        assert!(
            projections.contains_key(key),
            "empty declared set must emit built-in {key:?}; got keys {:?}",
            projections.keys().collect::<Vec<_>>()
        );
    }
}

/// Non-empty declared set narrows to its members: declared keys present,
/// undeclared keys absent. `relay_diagnostics` (the headline) is NOT declared
/// here and must be omitted.
#[test]
fn declared_set_narrows_to_members_and_omits_relay_diagnostics() {
    let (mut kernel, slot) = kernel_with_slot();
    slot.lock()
        .unwrap()
        .declare_consumed_projections(["profile", "accounts", "resolved_profiles"]);

    let projections = projections_json(&mut kernel);

    // Declared keys present.
    assert!(
        projections.contains_key("profile"),
        "declared `profile` present"
    );
    assert!(
        projections.contains_key("accounts"),
        "declared `accounts` present"
    );
    assert!(
        projections.contains_key("resolved_profiles"),
        "declared `resolved_profiles` present"
    );

    // THE acceptance criterion: relay_diagnostics is gated out.
    assert!(
        !projections.contains_key("relay_diagnostics"),
        "undeclared `relay_diagnostics` must NOT be serialized (ADR-0053 headline); \
         got keys {:?}",
        projections.keys().collect::<Vec<_>>()
    );
    // Other undeclared built-ins gated out too.
    for key in [
        "publish_queue",
        "settings_hub",
        "claimed_profiles",
        "active_account",
    ] {
        assert!(
            !projections.contains_key(key),
            "undeclared built-in {key:?} must be omitted; got keys {:?}",
            projections.keys().collect::<Vec<_>>()
        );
    }
}

/// The gate applies to the TYPED sidecar identically to the JSON map (ADR-0037
/// divergence-safety): a declared key's typed entry is present; an undeclared
/// key's typed entry is absent.
#[test]
fn declared_set_gates_typed_sidecar_in_lockstep_with_json() {
    let (mut kernel, slot) = kernel_with_slot();
    slot.lock()
        .unwrap()
        .declare_consumed_projections(["profile", "configured_relays"]);

    let (value, typed) = kernel.make_update_typed_for_test(true);
    let json_keys: std::collections::BTreeSet<String> = value
        .get("projections")
        .and_then(|p| p.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let typed_keys: std::collections::BTreeSet<String> =
        typed.iter().map(|d| d.key.clone()).collect();

    // Declared keys: present in both wire forms.
    for key in ["profile", "configured_relays"] {
        assert!(json_keys.contains(key), "declared {key:?} in JSON map");
        assert!(
            typed_keys.contains(key),
            "declared {key:?} in typed sidecar"
        );
    }
    // Undeclared `relay_diagnostics`: absent in both.
    assert!(
        !json_keys.contains("relay_diagnostics"),
        "undeclared relay_diagnostics absent from JSON map"
    );
    assert!(
        !typed_keys.contains("relay_diagnostics"),
        "undeclared relay_diagnostics absent from typed sidecar (parity with JSON)"
    );
}

/// A Tier-1 host-registered projection is NOT gated by the declared set — it
/// self-gates by registration. It surfaces even when the declared set is
/// non-empty and does not name it.
#[test]
fn tier1_host_projection_is_not_gated_by_declared_set() {
    let (mut kernel, slot) = kernel_with_slot();
    {
        let mut registry = slot.lock().unwrap();
        // Declare a narrow Tier-2 set that does NOT include the host key.
        registry.declare_consumed_projections(["profile"]);
        // Register a Tier-1 host projection (registration IS the declaration).
        registry.register("market.listings", || serde_json::json!([{ "id": "a" }]));
    }

    let projections = projections_json(&mut kernel);
    assert!(
        projections.contains_key("market.listings"),
        "Tier-1 host projection self-gates by registration and is NOT subject to \
         the Tier-2 declared-set gate; got keys {:?}",
        projections.keys().collect::<Vec<_>>()
    );
    // The Tier-2 `profile` is declared → present; `relay_diagnostics` not → absent.
    assert!(projections.contains_key("profile"));
    assert!(!projections.contains_key("relay_diagnostics"));
}

/// Declarations are additive: two `declare_consumed_projections` calls union.
#[test]
fn declarations_union_additively() {
    let (mut kernel, slot) = kernel_with_slot();
    {
        let mut registry = slot.lock().unwrap();
        registry.declare_consumed_projections(["profile"]);
        registry.declare_consumed_projections(["accounts"]);
    }
    let projections = projections_json(&mut kernel);
    assert!(projections.contains_key("profile"));
    assert!(projections.contains_key("accounts"));
    assert!(!projections.contains_key("settings_hub"));
}

/// The drain-on-emit keys still work when declared: a settled action result
/// surfaces under `action_results` on the tick it settles, and is omitted (but
/// still drained, no carryover) when undeclared.
#[test]
fn declared_drain_on_emit_key_surfaces_when_settled() {
    // Declared: action_results appears when a terminal settles.
    let (mut kernel, slot) = kernel_with_slot();
    slot.lock()
        .unwrap()
        .declare_consumed_projections(["action_results"]);
    kernel.record_action_success(
        "corr-1".to_string(),
        Some(r#"{"event_id":"a"}"#.to_string()),
    );
    let projections = projections_json(&mut kernel);
    assert!(
        projections.contains_key("action_results"),
        "declared action_results must surface on the settle tick; got {:?}",
        projections.keys().collect::<Vec<_>>()
    );

    // Undeclared: the same settle does NOT surface action_results, but the
    // source is still drained (the NEXT tick is clean, no carryover).
    let (mut kernel2, slot2) = kernel_with_slot();
    slot2
        .lock()
        .unwrap()
        .declare_consumed_projections(["profile"]); // narrowing, excludes action_results
    kernel2.record_action_success(
        "corr-2".to_string(),
        Some(r#"{"event_id":"b"}"#.to_string()),
    );
    let p2 = projections_json(&mut kernel2);
    assert!(
        !p2.contains_key("action_results"),
        "undeclared action_results must be omitted even on a settle tick"
    );
    // Next tick: still clean (the drain happened despite being undeclared).
    let p2_next = projections_json(&mut kernel2);
    assert!(
        !p2_next.contains_key("action_results"),
        "drain happened despite undeclared — no carryover into the next tick"
    );
}
