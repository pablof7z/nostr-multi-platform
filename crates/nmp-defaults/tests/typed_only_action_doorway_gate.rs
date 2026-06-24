//! Typed-only byte-doorway gate for the FULL production composition
//! (ADR-0064 / #1756).
//!
//! The byte doorway (`nmp_app_dispatch_action_bytes` →
//! `ActionRegistry::start` / `start_bytes`) is TYPED-ONLY: every action module
//! reachable through it MUST decode a typed FlatBuffers payload (override
//! `ActionModule::decode_payload` to return `Some`). A JSON-only /
//! no-`decode_payload` module is rejected `NotTypedCapable` and so is unreachable
//! through the byte doorway — this is the fail-closed invariant restored when the
//! opaque-passthrough JSON-compat shim (#1828) was reverted.
//!
//! The kernel-slice gate (`nmp-core` `default_registry`) only sees
//! `nmp.publish`. This gate spins up a REAL [`NmpApp`] and runs the actual
//! production composition root (`nmp_defaults::register_defaults`) — the same
//! call Chirp and external embedders make — so it covers EVERY canonical module
//! the NIP crates register (NIP-02/17/25/51/57/65, …). Pairs with a
//! load-bearing negative that registers a deliberately JSON-only module and
//! proves the gate flags it (so the assertion can never be vacuous).
//!
//! # Migration ratchet (ADR-0064 is per-crate, in-flight)
//!
//! ADR-0064 migrates each action crate to a typed FlatBuffers payload
//! INDIVIDUALLY; the JSON doorway (`nmp_app_dispatch_action`) still exists for
//! not-yet-migrated modules and is deleted only at Cut B. So a handful of
//! production modules legitimately remain JSON-only on the JSON doorway today.
//! Rather than assert ZERO untyped modules (which would falsely fail on those
//! in-flight modules), this gate pins the untyped set to a frozen ALLOWLIST.
//! The allowlist is a RATCHET:
//! * a NEW untyped module (not on the list) → FAILS the gate (regression: no
//!   one may add a JSON-only module without an explicit, reviewed allowlist
//!   entry, and re-adding the reverted opaque-passthrough shim does not help —
//!   the byte doorway still rejects these `NotTypedCapable`);
//! * migrating a listed module to typed without removing it from the list →
//!   FAILS the gate (forcing the allowlist to SHRINK toward empty as ADR-0064
//!   completes; at Cut B the JSON doorway and this allowlist both reach zero).

use nmp_ffi::{nmp_app_free, nmp_app_new};

/// Production modules NOT yet migrated to a typed FlatBuffers payload — they
/// ride the JSON doorway (`nmp_app_dispatch_action`) only and are rejected
/// `NotTypedCapable` by the byte doorway. This is the ADR-0064 migration
/// backlog. It MUST only shrink: each removal is a crate that finished its
/// typed migration.
///
/// As of #1756 this allowlist is EMPTY: the last three pending modules — the
/// `nmp-router`-owned `nmp.nip51.block_relay`, `nmp.nip51.unblock_relay`, and
/// `nmp.nip65.publish_relay_list` — are now typed, so EVERY canonical default
/// module decodes a typed FlatBuffers payload. The gate below now effectively
/// asserts "the untyped set is empty", the Cut-B end state: ADR-0064 Cut B can
/// delete the JSON doorway and this whole gate collapses to that assertion. A
/// re-grown allowlist entry would be a regression.
const MIGRATION_PENDING_UNTYPED: &[&str] = &[];

/// THE production gate: after the canonical `register_defaults` wiring, the
/// untyped (JSON-doorway-only) module set is EXACTLY the frozen migration
/// allowlist — no more (no new JSON-only module / no re-grown opaque shim), no
/// fewer (a migrated module must be struck from the allowlist). Everything else
/// is typed (ADR-0064 / #1756 — the byte doorway is typed-only).
#[test]
fn register_defaults_untyped_modules_match_the_migration_allowlist() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");
    // SAFETY: `app` is a valid non-null pointer fresh from `nmp_app_new`.
    let app_mut = unsafe { &mut *app };

    nmp_defaults::register_defaults(app_mut);

    let untyped = app_mut.untyped_action_namespaces(); // already sorted
    let mut expected: Vec<String> = MIGRATION_PENDING_UNTYPED
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    expected.sort();

    assert_eq!(
        untyped, expected,
        "the untyped (JSON-doorway-only) action-module set must equal the \
         frozen ADR-0064 migration allowlist. A namespace present here but NOT \
         in the allowlist is a NEW JSON-only module (forbidden — the byte \
         doorway is typed-only, #1756; re-adding the reverted #1828 \
         opaque-passthrough shim does not make it reachable). A namespace in the \
         allowlist but absent here finished its typed migration — strike it from \
         `MIGRATION_PENDING_UNTYPED` so the ratchet shrinks toward empty."
    );

    nmp_app_free(app);
}

/// A deliberately JSON-only module — `serde_json::Value` action, NO
/// `decode_payload` override. Reachable through the byte doorway only if the
/// reverted opaque-passthrough shim returns; the typed-only gate must flag it.
struct JsonOnlyAppModule;
impl nmp_core::substrate::ActionModule for JsonOnlyAppModule {
    const NAMESPACE: &'static str = "test.json_only_gate"; // doctrine-allow: action_namespace — test-only namespace inside a #[cfg(test)] integration test; never on the wire
    type Action = serde_json::Value;
    // `decode_payload` left defaulted (`None`) — the forbidden JSON-only shim.

    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// LOAD-BEARING negative: register a JSON-only module on a real app and prove
/// the gate FLAGS its namespace. If `untyped_action_namespaces()` (or the
/// underlying `is_typed_capable` probe) ever stopped distinguishing typed from
/// JSON-only modules, this would go green-when-it-should-be-red and the positive
/// gate above would be vacuous. Together they prove the gate fires iff a
/// JSON-only module is reachable through the byte doorway.
#[test]
fn gate_flags_a_json_only_module_registered_on_a_real_app() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");
    // SAFETY: valid non-null pointer fresh from `nmp_app_new`.
    let app_mut = unsafe { &mut *app };

    nmp_defaults::register_defaults(app_mut);
    let before = app_mut.untyped_action_namespaces();
    assert!(
        !before.contains(&"test.json_only_gate".to_string()),
        "precondition: the test JSON-only namespace is not yet registered"
    );

    // Introduce the forbidden JSON-only shim.
    let _ = app_mut.register_action(JsonOnlyAppModule);

    let after = app_mut.untyped_action_namespaces();
    assert!(
        after.contains(&"test.json_only_gate".to_string()),
        "the typed-only gate must flag the JSON-only module's namespace \
         (proving it is load-bearing, not vacuous); got: {after:?}"
    );
    assert_eq!(
        after.len(),
        before.len() + 1,
        "registering one JSON-only module must add EXACTLY one untyped namespace"
    );

    nmp_app_free(app);
}
