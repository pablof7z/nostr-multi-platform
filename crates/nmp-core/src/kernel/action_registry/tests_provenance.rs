//! ADR-0049 Part 1 — directional registry semantics (order-independent yield)
//! provenance tests. Extracted from `tests.rs` to keep that file under its
//! baseline LOC ceiling. Loaded via `#[path = "tests_provenance.rs"] mod
//! adr_0049_yield;` from within `tests.rs`.

use super::*;
use crate::kernel::composition_ledger::{CompositionLedger, Disposition};
use std::sync::Arc;

/// Two distinct modules that claim the SAME namespace, so we can observe
/// which one wins after a yield/override. They differ only by type identity.
struct DefaultModule;
impl ActionModule for DefaultModule {
    type Action = serde_json::Value;
    const NAMESPACE: &'static str = "nmp.test.adr0049.ns";
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

struct AppModule;
impl ActionModule for AppModule {
    type Action = serde_json::Value;
    const NAMESPACE: &'static str = "nmp.test.adr0049.ns";
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// A second app module under a DIFFERENT namespace, used for the
/// no-collision happy path.
struct OtherAppModule;
impl ActionModule for OtherAppModule {
    type Action = serde_json::Value;
    const NAMESPACE: &'static str = "nmp.test.adr0049.other";
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn default_then_app_app_wins() {
    let mut registry = ActionRegistry::new();
    assert!(
        registry.register_default(DefaultModule),
        "first default install returns true"
    );
    registry.register(AppModule);
    assert!(registry.contains("nmp.test.adr0049.ns"));
}

#[test]
fn app_then_default_app_wins() {
    // App registers first; the later default must YIELD.
    let mut registry = ActionRegistry::new();
    registry.register(AppModule);
    let installed = registry.register_default(DefaultModule);
    assert!(
        !installed,
        "default must yield (return false) when the namespace is already claimed by an app"
    );
    assert!(registry.contains("nmp.test.adr0049.ns"));
}

#[test]
fn default_then_default_first_default_wins() {
    let mut registry = ActionRegistry::new();
    assert!(registry.register_default(DefaultModule));
    assert!(
        !registry.register_default(AppModule),
        "a second default under the same namespace yields"
    );
}

#[test]
fn ledger_records_install_then_yield_with_provider() {
    let ledger = Arc::new(CompositionLedger::new());
    let mut registry = ActionRegistry::new().with_composition_ledger(Arc::clone(&ledger));

    registry.register(AppModule);
    assert!(!registry.register_default(DefaultModule));

    let records = ledger.records();
    assert_eq!(records.len(), 2);

    assert_eq!(records[0].seam, "action_registry");
    assert_eq!(records[0].key, "nmp.test.adr0049.ns");
    assert_eq!(records[0].disposition, Disposition::Installed);
    assert!(records[0].provider.contains("AppModule"));
    assert!(records[0].replaced.is_none());

    assert_eq!(records[1].disposition, Disposition::YieldedToExisting);
    assert!(records[1].provider.contains("DefaultModule"));
    assert!(
        records[1]
            .replaced
            .as_deref()
            .map(|p| p.contains("AppModule"))
            .unwrap_or(false),
        "yield record names the existing app provider it yielded to"
    );
}

#[test]
fn ledger_records_app_over_default_as_replaced() {
    let ledger = Arc::new(CompositionLedger::new());
    let mut registry = ActionRegistry::new().with_composition_ledger(Arc::clone(&ledger));

    registry.register_default(DefaultModule);
    registry.register(AppModule);

    let records = ledger.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].disposition, Disposition::Installed);
    assert_eq!(records[1].disposition, Disposition::ReplacedPrevious);
    assert!(
        records[1]
            .replaced
            .as_deref()
            .map(|p| p.contains("DefaultModule"))
            .unwrap_or(false),
        "app-over-default replace names the default it replaced"
    );
}

#[test]
fn distinct_namespaces_both_install_no_collision() {
    let ledger = Arc::new(CompositionLedger::new());
    let mut registry = ActionRegistry::new().with_composition_ledger(Arc::clone(&ledger));
    registry.register(AppModule);
    registry.register(OtherAppModule);
    let records = ledger.records();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|r| r.disposition == Disposition::Installed));
    assert!(registry.contains("nmp.test.adr0049.ns"));
    assert!(registry.contains("nmp.test.adr0049.other"));
}

// App-over-app collision behaviour: in dev/test builds (`debug_assertions`
// on) a second app registration under the same namespace fires a
// `debug_assert!` and panics. In release the same path is a soft
// last-writer-wins (ReplacedPrevious).
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "composition collision")]
fn app_over_app_collision_panics_in_dev() {
    let mut registry = ActionRegistry::new();
    registry.register(AppModule);
    registry.register(OtherAppModuleSameNs);
}

#[cfg(debug_assertions)]
struct OtherAppModuleSameNs;
#[cfg(debug_assertions)]
impl ActionModule for OtherAppModuleSameNs {
    type Action = serde_json::Value;
    const NAMESPACE: &'static str = "nmp.test.adr0049.ns";
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}
