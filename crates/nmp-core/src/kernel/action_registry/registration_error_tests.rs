//! Tests for the structured `RegistrationError` returned on app-over-app
//! namespace collisions (issue #1724).

use super::*;
use crate::actor::ActorCommand;

struct AppModule;
impl ActionModule for AppModule {
    type Action = serde_json::Value;
    const NAMESPACE: &'static str = "nmp.test.reg_err.ns";
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

struct OtherAppModuleSameNs;
impl ActionModule for OtherAppModuleSameNs {
    type Action = serde_json::Value;
    const NAMESPACE: &'static str = "nmp.test.reg_err.ns";
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

// App-over-app collision behaviour (#1724): `register` now returns a
// structured `RegistrationError` in BOTH dev AND release — no longer a
// `debug_assert!` that silently disappears in production. The new module
// still wins (last-writer-wins for release resilience, D6).
#[test]
fn app_over_app_collision_returns_structured_error() {
    let mut registry = ActionRegistry::new();
    assert!(
        registry.register(AppModule).is_ok(),
        "first registration must succeed"
    );
    let err = registry
        .register(OtherAppModuleSameNs)
        .expect_err("second app registration on same namespace must return RegistrationError");
    assert_eq!(err.namespace, "nmp.test.reg_err.ns");
    assert!(
        err.prior_provider.contains("AppModule"),
        "prior_provider should name AppModule, got: {}",
        err.prior_provider
    );
    assert!(
        err.new_provider.contains("OtherAppModuleSameNs"),
        "new_provider should name OtherAppModuleSameNs, got: {}",
        err.new_provider
    );
    // Last-writer-wins (D6 — no panic across the C-ABI):
    assert!(
        registry.contains("nmp.test.reg_err.ns"),
        "namespace still present after collision"
    );
}
