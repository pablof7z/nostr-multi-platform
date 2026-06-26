use super::*;

fn ctx() -> ActionContext {
    ActionContext::default()
}

#[test]
fn deliver_result_invokes_registered_observer() {
    use std::sync::{Arc, Mutex};
    let seen: Arc<Mutex<Vec<ActionResult>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_in_observer = Arc::clone(&seen);

    let registry = default_registry();
    registry.set_result_observer(move |result| {
        seen_in_observer.lock().unwrap().push(result);
    });

    registry.deliver_result(ActionResult {
        correlation_id: "abc123".to_string(),
        result_json: serde_json::Value::Null,
    });

    let captured = seen.lock().unwrap();
    assert_eq!(captured.len(), 1, "observer should be called exactly once");
    assert_eq!(
        captured[0].correlation_id, "abc123",
        "observer should receive the delivered correlation id"
    );
    assert!(
        captured[0].result_json.is_null(),
        "fire-and-forget delivery carries a null result_json"
    );
}

#[test]
fn deliver_result_without_observer_is_silent_noop() {
    let registry = default_registry();
    registry.deliver_result(ActionResult {
        correlation_id: "no-observer".to_string(),
        result_json: serde_json::Value::Null,
    });
}

#[test]
fn set_result_observer_second_registration_replaces_first() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    let first = Arc::new(AtomicU32::new(0));
    let second = Arc::new(AtomicU32::new(0));
    let first_c = Arc::clone(&first);
    let second_c = Arc::clone(&second);

    let registry = default_registry();
    registry.set_result_observer(move |_| {
        first_c.fetch_add(1, Ordering::SeqCst);
    });
    registry.set_result_observer(move |_| {
        second_c.fetch_add(1, Ordering::SeqCst);
    });

    registry.deliver_result(ActionResult {
        correlation_id: "x".to_string(),
        result_json: serde_json::Value::Null,
    });

    assert_eq!(
        first.load(Ordering::SeqCst),
        0,
        "first observer is replaced"
    );
    assert_eq!(
        second.load(Ordering::SeqCst),
        1,
        "second observer receives it"
    );
}

#[test]
fn correlation_ids_are_unique_across_calls() {
    let registry = default_registry();
    let action_json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"x","target":"Auto"}}"#;
    let mut seen = std::collections::HashSet::new();
    for _ in 0..256 {
        let id = registry
            .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", action_json)
            .unwrap();
        assert!(seen.insert(id.clone()), "duplicate correlation id: {id}");
    }
}

/// D6 — a typed [`ActionModule::start`] that panics is contained:
/// `start` returns [`ActionRejection::Invalid`] instead of unwinding
/// across the FFI boundary.
#[test]
fn panicking_validator_is_rejected_not_unwound() {
    struct PanickingStartModule;
    impl ActionModule for PanickingStartModule {
        const NAMESPACE: &'static str = "host.boom_start"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
        type Action = serde_json::Value;
        fn start(
            &self,
            _ctx: &mut ActionContext,
            _action: Self::Action,
        ) -> Result<(), ActionRejection> {
            panic!("buggy module validator");
        }
        fn execute(
            &self,
            _ctx: &ActionContext,
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            Ok(())
        }
    }

    let mut registry = ActionRegistry::new();
    let _ = registry.register(PanickingStartModule);
    let err = registry
        .start(&mut ctx(), 1_700_000_000_000, "host.boom_start", "null")
        .expect_err("a panicking validator must be rejected, not unwound");
    match err {
        ActionRejection::Invalid(msg) => {
            assert_eq!(msg, "action validator panicked", "got: {msg}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

/// D6 — a typed [`ActionModule::execute`] that panics is contained:
/// `execute` returns `Err` instead of unwinding.
///
/// `execute` is reached from `nmp_app_dispatch_action` (an `extern "C"` fn), so
/// an unguarded panic would unwind across the FFI boundary. The registry wraps
/// every typed-module call in [`catch_unwind`] (`ActionRegistry::execute`);
/// without it this test would panic out rather than returning `Err`.
#[test]
fn panicking_executor_returns_err_not_unwound() {
    struct PanickingExecuteModule;
    impl ActionModule for PanickingExecuteModule {
        const NAMESPACE: &'static str = "host.boom"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
        type Action = serde_json::Value;
        fn start(
            &self,
            _ctx: &mut ActionContext,
            _action: Self::Action,
        ) -> Result<(), ActionRejection> {
            Ok(())
        }
        fn execute(
            &self,
            _ctx: &ActionContext,
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            panic!("buggy module executor");
        }
    }

    let mut registry = ActionRegistry::new();
    let _ = registry.register(PanickingExecuteModule);
    let err = registry
        .execute(&ctx(), "host.boom", "null", "corr-id", &|_cmd| {})
        .expect_err("a panicking executor must return Err, not unwind");
    assert_eq!(err.message, "action executor panicked", "got: {err:?}");
}

/// D6 — a host result-observer closure that panics is contained:
/// `deliver_result` swallows the unwind and the observer stays registered so
/// the next result is still delivered. The observer is untrusted host plugin
/// code (`nmp_app_register_action_result_observer`) running on the FFI dispatch
/// thread; an unguarded panic would poison the slot mutex AND unwind across the
/// FFI boundary. The `catch_unwind` guard turns it into a per-result drop.
#[test]
fn panicking_result_observer_does_not_kill_delivery() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    let calls = Arc::new(AtomicU32::new(0));
    let calls_in_observer = Arc::clone(&calls);

    let registry = default_registry();
    registry.set_result_observer(move |result| {
        let n = calls_in_observer.fetch_add(1, Ordering::SeqCst) + 1;
        if n == 1 {
            panic!(
                "buggy host result observer (call #{}, corr={})",
                n, result.correlation_id
            );
        }
    });

    registry.deliver_result(ActionResult {
        correlation_id: "first".to_string(),
        result_json: serde_json::Value::Null,
    });
    registry.deliver_result(ActionResult {
        correlation_id: "second".to_string(),
        result_json: serde_json::Value::Null,
    });

    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "observer must have been invoked twice — once panicking, once successfully"
    );
}

// ADR-0049 Part 1 — directional registry semantics (order-independent yield).
mod adr_0049_yield {
    use super::*;
    use crate::kernel::composition_ledger::{CompositionLedger, Disposition};
    use std::sync::Arc;

    struct DefaultModule;
    impl ActionModule for DefaultModule {
        type Action = serde_json::Value;
        const NAMESPACE: &'static str = "nmp.test.adr0049.ns";
        fn execute(
            &self,
            _ctx: &ActionContext,
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
            _ctx: &ActionContext,
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            Ok(())
        }
    }

    struct OtherAppModule;
    impl ActionModule for OtherAppModule {
        type Action = serde_json::Value;
        const NAMESPACE: &'static str = "nmp.test.adr0049.other";
        fn execute(
            &self,
            _ctx: &ActionContext,
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
        let _ = registry.register(AppModule);
        assert!(registry.contains("nmp.test.adr0049.ns"));
    }

    #[test]
    fn app_then_default_app_wins() {
        let mut registry = ActionRegistry::new();
        let _ = registry.register(AppModule);
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

        let _ = registry.register(AppModule);
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
        let _ = registry.register(AppModule);

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
        let _ = registry.register(AppModule);
        let _ = registry.register(OtherAppModule);
        let records = ledger.records();
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(|r| r.disposition == Disposition::Installed));
        assert!(registry.contains("nmp.test.adr0049.ns"));
        assert!(registry.contains("nmp.test.adr0049.other"));
    }
}
