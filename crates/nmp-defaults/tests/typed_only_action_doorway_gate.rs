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
//! The allowed untyped set is no longer a hand-maintained allowlist. It derives
//! from `ActionContract::typed_dispatch`, so a JSON-only default action needs an
//! explicit tracked exemption in the contract and this gate fails on drift.

mod common;
use common::*;

/// THE production gate: after the canonical `register_defaults` wiring, the
/// untyped (JSON-doorway-only) module set is EXACTLY the contract exemption
/// set. Everything else is typed (ADR-0064 / #1756).
#[test]
fn register_defaults_untyped_modules_match_the_migration_allowlist() {
    let app = new_app_ptr();
    assert!(!app.is_null(), "nmp_app_new returned null");
    // SAFETY: `app` is a valid non-null pointer fresh from `nmp_app_new`.
    let app_mut = unsafe { &mut *app };

    nmp_defaults::register_defaults(app_mut);

    let untyped = app_mut.untyped_action_namespaces(); // already sorted
    let expected: Vec<String> = nmp_codegen::typed_dispatch_exemption_namespaces()
        .into_iter()
        .map(str::to_string)
        .collect();

    assert_eq!(
        untyped, expected,
        "the untyped (JSON-doorway-only) action-module set must equal \
         ACTION_CONTRACT typed-dispatch exemptions. A namespace present here \
         without an exemption is a forbidden JSON-only default module; an \
         exemption absent here must be removed from the contract."
    );

    free_app_ptr(app);
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
        _ctx: &nmp_core::substrate::ActionContext,
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
    let app = new_app_ptr();
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

    free_app_ptr(app);
}
