//! ADR-0070 — host-declared projection subscriptions: end-to-end gating proofs.
//!
//! These drive `make_update` (the path the host actually consumes) and assert
//! WHICH Tier-2 built-in projection keys appear in the emitted snapshot under
//! the host-declared consumed-projection set:
//!
//! - empty declared set ⇒ every Tier-2 built-in is present (no narrowing);
//! - non-empty declared set ⇒ only declared keys present, undeclared omitted;
//! - the gate applies to BOTH the generic JSON map and the typed sidecar (the
//!   ADR-0072 divergence-safety invariant extended to the gate);
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
/// as before ADR-0070. This is the "host expressed no opinion" semantic and the
/// guarantee that the kernel's own Rust consumers / test helpers keep working
/// with zero declaration.
#[test]
fn empty_declared_set_emits_all_builtins() {
    let (mut kernel, _slot) = kernel_with_slot();
    let projections = projections_json(&mut kernel);

    // The unconditional Tier-2 built-ins must all be present (the drain-on-emit
    // four are absent in steady state — that is their own convention, unrelated
    // to the declared-set gate).
    // ADR-0070 Lane H: mention_profiles / claimed_profiles / resolved_profiles deleted.
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
///
/// ADR-0070 Lane H: resolved_profiles / claimed_events JSON projections deleted;
/// use live JSON built-ins here. refs.event declaration semantics are covered by
/// the keyed row-delta integration tests.
#[test]
fn declared_set_narrows_to_members_and_omits_relay_diagnostics() {
    let (mut kernel, slot) = kernel_with_slot();
    slot.lock()
        .unwrap()
        .declare_consumed_projections(["profile", "accounts", "configured_relays"]);

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
        projections.contains_key("configured_relays"),
        "declared `configured_relays` present"
    );

    // THE acceptance criterion: relay_diagnostics is gated out.
    assert!(
        !projections.contains_key("relay_diagnostics"),
        "undeclared `relay_diagnostics` must NOT be serialized (ADR-0070 headline); \
         got keys {:?}",
        projections.keys().collect::<Vec<_>>()
    );
    // Other undeclared built-ins gated out too.
    for key in ["publish_queue", "settings_hub", "active_account"] {
        assert!(
            !projections.contains_key(key),
            "undeclared built-in {key:?} must be omitted; got keys {:?}",
            projections.keys().collect::<Vec<_>>()
        );
    }
    // ADR-0070 Lane H: these deleted projections must never appear.
    for key in ["claimed_profiles", "mention_profiles", "resolved_profiles"] {
        assert!(
            !projections.contains_key(key),
            "deleted projection `{key}` must never appear (ADR-0070 Lane H)"
        );
    }
}

/// The gate applies to the TYPED sidecar identically to the JSON map (ADR-0072
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

/// A Tier-1 host-registered typed projection is NOT gated by the declared set
/// — it self-gates by registration. The typed sidecar entry surfaces even when
/// the declared set is non-empty and does not name the host key.
#[test]
fn tier1_host_projection_is_not_gated_by_declared_set() {
    use crate::update_envelope::TypedProjectionData;
    let (mut kernel, slot) = kernel_with_slot();
    {
        let mut registry = slot.lock().unwrap();
        // Declare a narrow Tier-2 set that does NOT include the host key.
        registry.declare_consumed_projections(["profile"]);
        // Register a Tier-1 typed host projection (registration IS the declaration).
        registry.register_typed("market.listings", || {
            Some(TypedProjectionData {
                key: "market.listings".into(),
                schema_id: "market".into(),
                schema_version: 1,
                file_identifier: "TEST".into(),
                payload: vec![1u8],
                ..Default::default()
            })
        });
    }

    // The typed sidecar carries the host key despite it being absent from the
    // Tier-2 declared set — Tier-1 host projections self-gate by registration.
    let (_value, typed) = kernel.make_update_typed_for_test(true);
    let host_key = typed.iter().find(|t| t.key == "market.listings");
    assert!(
        host_key.is_some(),
        "Tier-1 typed host projection must surface in the typed sidecar regardless \
         of the declared-set gate; typed keys: {:?}",
        typed.iter().map(|t| &t.key).collect::<Vec<_>>()
    );
    // The Tier-2 `profile` is declared → present in JSON map;
    // `relay_diagnostics` not declared → absent.
    let projections = projections_json(&mut kernel);
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

// ── Workstream-E3 — declared ⊆ decodable drift gate (chokepoint enforcement) ──

/// **Green on master.** Declaring real kernel built-ins through the registry
/// chokepoint does not trip the drift gate (no panic). This is the shape every
/// real host declaration takes — Chirp declares only `KERNEL_BUILTIN_PROJECTION_KEYS`
/// members — so the gate must be silent for them.
#[test]
fn declaring_only_builtins_does_not_trip_the_drift_gate() {
    use crate::kernel::snapshot_registry::SnapshotRegistry;
    let mut registry = SnapshotRegistry::new();
    // Declare the full built-in set — the most a host could legitimately consume.
    registry.declare_consumed_projections(
        crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS
            .iter()
            .map(|k| k.to_string()),
    );
    assert!(registry.declared_projections().is_narrowing());
}

/// **Non-vacuity.** A declaration through the registry chokepoint that names a
/// key the framework never emits (here a typo of `relay_diagnostics`) trips the
/// `debug_assert!` drift gate. Gated on `debug_assertions` because the
/// behaviour-preserving release path replaces the assert with a non-fatal
/// `tracing::warn!` (the runtime is never failed in release).
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "declared consumed-projection key(s) not in")]
fn declaring_a_non_builtin_trips_the_drift_gate_in_debug() {
    use crate::kernel::snapshot_registry::SnapshotRegistry;
    let mut registry = SnapshotRegistry::new();
    // `relay_diagnstics` is a typo of the real `relay_diagnostics` — exactly the
    // silent-drift hazard E3 closes: a non-decodable key that flips the set into
    // narrowing mode and drops the real key from every frame.
    registry.declare_consumed_projections(["profile", "relay_diagnstics"]);
}
