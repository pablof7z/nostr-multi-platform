use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use super::super::{nmp_app_free, nmp_app_new};
use super::*;

fn with_app(body: impl FnOnce(&NmpApp)) {
    let app = nmp_app_new();
    body(unsafe { &*app });
    nmp_app_free(app);
}

static GREETING_CALLED: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();

fn greeting_flag() -> Arc<AtomicBool> {
    GREETING_CALLED
        .get_or_init(|| Arc::new(AtomicBool::new(false)))
        .clone()
}

struct TestGreetingModule;
impl nmp_core::substrate::ActionModule for TestGreetingModule {
    const NAMESPACE: &'static str = "test.greeting"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
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
        _ctx: &nmp_core::substrate::ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
    ) -> Result<(), String> {
        greeting_flag().store(true, Ordering::SeqCst);
        Ok(())
    }
}

struct TestFailingModule;
impl nmp_core::substrate::ActionModule for TestFailingModule {
    const NAMESPACE: &'static str = "test.failing"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
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
        _ctx: &nmp_core::substrate::ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
    ) -> Result<(), String> {
        Err("host rejected the action".to_string())
    }
}

struct TestTodoModule;
impl nmp_core::substrate::ActionModule for TestTodoModule {
    const NAMESPACE: &'static str = "test.todo"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
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
        _ctx: &nmp_core::substrate::ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

struct TestTodoRejectModule;
impl nmp_core::substrate::ActionModule for TestTodoRejectModule {
    const NAMESPACE: &'static str = "test.todo_reject"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
    type Action = serde_json::Value;
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Err(ActionRejection::Invalid(
            "host rejected: title required".into(),
        ))
    }
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

struct TestPanicModule;
impl nmp_core::substrate::ActionModule for TestPanicModule {
    const NAMESPACE: &'static str = "test.panic"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
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
        _ctx: &nmp_core::substrate::ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
    ) -> Result<(), String> {
        panic!("buggy executor")
    }
}

#[test]
fn host_registered_executor_dispatches_successfully() {
    greeting_flag().store(false, Ordering::SeqCst);

    let app = nmp_app_new();
    let app_mut = unsafe { &mut *app };
    let _ = app_mut.register_action(TestGreetingModule);

    app_mut
        .test_execute_action("test.greeting", r#"{"hello":"world"}"#)
        .expect("host-registered executor should run");

    assert!(
        greeting_flag().load(Ordering::SeqCst),
        "host-registered executor was never invoked"
    );
    nmp_app_free(app);
}

#[test]
fn host_registered_executor_propagates_error() {
    let app = nmp_app_new();
    let app_mut = unsafe { &mut *app };
    let _ = app_mut.register_action(TestFailingModule);

    let err = app_mut
        .test_execute_action("test.failing", "{}")
        .expect_err("a failing host executor must surface an error");
    assert_eq!(err, "host rejected the action");
    nmp_app_free(app);
}

#[test]
fn unregistered_namespace_after_host_registration_still_errs() {
    let app = nmp_app_new();
    let app_mut = unsafe { &mut *app };
    let _ = app_mut.register_action(TestGreetingModule);

    let err = app_mut
        .test_execute_action("test.unregistered", "{}")
        .expect_err("an unregistered namespace must still error");
    assert!(
        err.contains("no executor registered") && err.contains("test.unregistered"),
        "error should name the unregistered namespace, got: {err}"
    );
    nmp_app_free(app);
}

#[test]
fn host_registered_module_and_executor_enables_dispatch_action() {
    let app = nmp_app_new();
    let app_mut = unsafe { &mut *app };
    let _ = app_mut.register_action(TestTodoModule);

    let out = dispatch_action_json(
        Some(&*app_mut),
        "test.todo",
        r#"{"create":{"title":"buy milk"}}"#,
    );
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        parsed.get("correlation_id").is_some(),
        "expected correlation_id, got: {out}"
    );
    nmp_app_free(app);
}

#[test]
fn host_registered_module_can_reject_action() {
    let app = nmp_app_new();
    let app_mut = unsafe { &mut *app };
    let _ = app_mut.register_action(TestTodoRejectModule);

    let out = dispatch_action_json(Some(&*app_mut), "test.todo_reject", "{}");
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected error object, got: {out}"));
    assert!(
        err.contains("host rejected: title required"),
        "rejection message should reach the host, got: {err}"
    );
    nmp_app_free(app);
}

#[test]
fn dispatch_action_delivers_result_to_observer_with_correlation_id() {
    let seen: Arc<Mutex<Vec<nmp_core::substrate::ActionResult>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_in_observer = Arc::clone(&seen);

    with_app(|app| {
        app.register_action_result_observer(move |result| {
            seen_in_observer.lock().unwrap().push(result);
        });

        let out = dispatch_action_json(
            Some(app),
            "nmp.publish",
            r#"{"PublishRaw":{"kind":1,"tags":[],"content":"observer-test","target":"Auto"}}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let returned_id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .expect("dispatch should return a correlation_id")
            .to_string();

        let captured = seen.lock().unwrap();
        assert_eq!(
            captured.len(),
            1,
            "the result observer should fire exactly once per accepted action"
        );
        assert_eq!(
            captured[0].correlation_id, returned_id,
            "observer correlation_id must match the dispatch return value"
        );
        assert!(
            captured[0].result_json.is_null(),
            "a fire-and-forget built-in executor delivers a null result_json"
        );
    });
}

#[test]
fn dispatch_action_does_not_deliver_result_on_rejection() {
    let fired = Arc::new(AtomicBool::new(false));
    let fired_in_observer = Arc::clone(&fired);

    with_app(|app| {
        app.register_action_result_observer(move |_| {
            fired_in_observer.store(true, Ordering::SeqCst);
        });
        let out = dispatch_action_json(Some(app), "nmp.unknown", "{}");
        assert!(
            out.contains("error"),
            "an unknown namespace must be rejected, got: {out}"
        );
        assert!(
            !fired.load(Ordering::SeqCst),
            "the observer must not fire for a rejected action"
        );
    });
}

#[test]
fn executor_failure_returns_correlation_id_and_enqueues_failed_terminal() {
    let app = nmp_app_new();
    let app_mut = unsafe { &mut *app };
    let _ = app_mut.register_action(TestPanicModule);

    let sends_before = app_mut.send_cmd_count_for_test();
    let out = dispatch_action_json(Some(&*app_mut), "test.panic", "{}");
    let sends_after = app_mut.send_cmd_count_for_test();

    let parsed: serde_json::Value =
        serde_json::from_str(&out).expect("dispatch envelope must be parseable JSON");
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!("executor failure envelope must include correlation_id; got: {out}")
        });
    assert_eq!(id.len(), 32, "correlation_id should still be 32 hex chars");
    let err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!("executor failure envelope must include error message; got: {out}")
        });
    assert!(
        err.contains("action executor panicked"),
        "error must surface the panic reason verbatim; got: {err}"
    );
    assert!(
        sends_after > sends_before,
        "executor failure must enqueue at least one ActorCommand \
         (RecordActionFailure); sends_before={sends_before} sends_after={sends_after}"
    );
    nmp_app_free(app);
}
