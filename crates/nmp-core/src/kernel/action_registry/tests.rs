use super::*;
use crate::substrate::{SignedEvent, UnsignedEvent};

fn ctx() -> ActionContext {
    ActionContext::default()
}

/// A `SignedEvent` with non-empty `id`/`sig` — enough to pass
/// `PublishModule::start`'s "requires a signed event" gate. The content
/// is irrelevant: `start` never inspects `unsigned`.
fn fixture_signed_event() -> SignedEvent {
    SignedEvent {
        id: "a".repeat(64),
        sig: "b".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: "c".repeat(64),
            kind: 1,
            tags: Vec::new(),
            content: "test".to_string(),
            created_at: 1_700_000_000,
        },
    }
}

#[test]
fn default_registry_has_publish_module() {
    let registry = default_registry();
    assert!(registry.contains("nmp.publish"));
    assert!(!registry.contains("nmp.nope"));
}

// V-38: the `nmp.wallet.pay_invoice` registration test moved to `nmp-nip47`
// (the crate that now owns `WalletPayInvoiceModule`). `default_registry`
// post-V-38 no longer registers it — host apps register the module
// themselves from `nmp-nip47`.

#[test]
fn start_publish_raw_action_returns_correlation_id() {
    // `PublishAction::PublishRaw` for a kind:1 note exercises the full
    // registry → adapter → module::start path without needing a fully-signed
    // event fixture. The actor signs the event, so `preferred_action_id`
    // returns `None` and the registry mints a random 32-hex-char
    // `correlation_id`.
    let registry = default_registry();
    let action_json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"hello","target":"Auto"}}"#;
    let id = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", action_json)
        .expect("publish raw action should be accepted");
    assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit()),
        "correlation id should be hex: {id}"
    );
}

#[test]
fn start_cancel_action_is_rejected_via_dispatch() {
    // Publish cancel is engine-internal — it is driven by the
    // `nmp_app_cancel_publish` FFI symbol, never `dispatch_action`.
    // `PublishModule::start` therefore rejects a `Cancel` action so the
    // generic action seam carries nothing for cancel.
    let registry = default_registry();
    let action_json = r#"{"Cancel":{"handle":"smoke-test"}}"#;
    let err = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", action_json)
        .expect_err("cancel must not be dispatchable via dispatch_action");
    match err {
        ActionRejection::Invalid(msg) => {
            assert!(
                msg.contains("nmp_app_cancel_publish"),
                "rejection should point at the FFI symbol: {msg}"
            );
        }
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

#[test]
fn start_publish_action_with_signed_event_is_accepted() {
    // A `PublishAction::Publish` with a non-empty id+sig passes
    // `PublishModule::start`'s validation gate.
    //
    // `preferred_action_id` returns the event's `id` (64 hex chars) so that
    // `dispatch_action`'s return value and `action_results` in the
    // snapshot share the same identifier. The fixture event has `id =
    // "a".repeat(64)` — 64 hex chars, not the 32-char minted `new_action_id`.
    let registry = default_registry();
    let event = fixture_signed_event();
    let expected_id = event.id.clone();
    let action = crate::publish::PublishAction::Publish {
        handle: "h1".to_string(),
        event,
        target: crate::publish::PublishTarget::Auto,
    };
    let action_json = serde_json::to_string(&action).unwrap();
    let id = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", &action_json)
        .expect("publish action with id+sig should be accepted");
    assert_eq!(
        id, expected_id,
        "Publish action must use event.id as correlation_id"
    );
}

#[test]
fn unknown_namespace_is_rejected() {
    let registry = default_registry();
    let err = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.does-not-exist", "{}")
        .expect_err("unknown namespace must be rejected");
    match err {
        ActionRejection::Invalid(msg) => {
            assert!(msg.contains("unknown action namespace"), "got: {msg}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn malformed_json_is_rejected_as_invalid() {
    let registry = default_registry();
    let err = registry
        .start(
            &mut ctx(),
            1_700_000_000_000,
            "nmp.publish",
            "{not valid json",
        )
        .expect_err("malformed JSON must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "expected Invalid, got {err:?}"
    );
}

#[test]
fn json_not_matching_action_shape_is_rejected() {
    // Valid JSON, wrong shape for `PublishAction` — serde's externally
    // tagged enum expects `{"<Variant>": {...}}`, so a flat
    // `{"t":"PublishRaw"}` matches no variant and is rejected.
    let registry = default_registry();
    let err = registry
        .start(
            &mut ctx(),
            1_700_000_000_000,
            "nmp.publish",
            r#"{"t":"PublishRaw"}"#,
        )
        .expect_err("wrong-shape JSON must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

/// THE FIX: the `nmp.publish` executor threads the registry-minted
/// `correlation_id` onto `ActorCommand::PublishRawEvent`. The actor signs the
/// event, so its id is unknown at dispatch time — without this, the
/// publish engine would report the event id and the host's spinner (keyed
/// on the dispatch return value) could never be cleared. This exercises
/// the real `default_registry()` executor closure end-to-end via
/// `execute()`, capturing the `ActorCommand` it sends.
#[test]
fn publish_raw_executor_threads_correlation_id_onto_actor_command() {
    use crate::actor::ActorCommand;
    use std::cell::RefCell;

    let registry = default_registry();
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());

    let minted_correlation_id = "fe".repeat(16);
    let action_json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"hello","target":{"Explicit":{"relays":["wss://relay.example"]}}}}"#;
    registry
        .execute("nmp.publish", action_json, &minted_correlation_id, &|cmd| {
            captured.borrow_mut().push(cmd);
        })
        .expect("publish-raw execution should succeed");

    let cmds = captured.into_inner();
    assert_eq!(
        cmds.len(),
        1,
        "executor must emit exactly one ActorCommand; got {cmds:?}"
    );
    match cmds.into_iter().next().unwrap() {
        ActorCommand::PublishRawEvent {
            kind,
            content,
            target,
            correlation_id,
            ..
        } => {
            assert_eq!(kind, 1);
            assert_eq!(content, "hello");
            assert_eq!(
                target,
                crate::publish::PublishTarget::Explicit {
                    relays: vec!["wss://relay.example".to_string()],
                },
                "the executor must preserve the validated publish target"
            );
            assert_eq!(
                correlation_id,
                Some(minted_correlation_id),
                "the executor must thread the minted correlation_id onto the command"
            );
        }
        other => panic!("expected ActorCommand::PublishRawEvent, got {other:?}"),
    }
}

/// The pre-signed `Publish` executor threads the registry-minted
/// `correlation_id` onto `ActorCommand::PublishSignedEvent` — explicit
/// symmetry with the `PublishRaw` path. The round-trip used to work by
/// coincidence (`preferred_action_id` returns `event.id`, the engine's
/// `None`-fallback also reports `event.id`); the explicit thread upgrades
/// that coincidence into a guarantee the publish engine surfaces the
/// dispatch-returned id even if future changes ever decouple the dispatch
/// return value from the publish handle.
#[test]
fn publish_signed_executor_sends_publish_signed_event_command() {
    use crate::actor::ActorCommand;
    use std::cell::RefCell;

    let registry = default_registry();
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());

    let action = crate::publish::PublishAction::Publish {
        handle: "h-presigned".to_string(),
        event: fixture_signed_event(),
        target: crate::publish::PublishTarget::Auto,
    };
    let action_json = serde_json::to_string(&action).unwrap();
    let minted_correlation_id = "ae".repeat(16);
    registry
        .execute(
            "nmp.publish",
            &action_json,
            &minted_correlation_id,
            &|cmd| {
                captured.borrow_mut().push(cmd);
            },
        )
        .expect("publish execution should succeed");

    let cmds = captured.into_inner();
    assert_eq!(
        cmds.len(),
        1,
        "executor must emit exactly one ActorCommand; got {cmds:?}"
    );
    match cmds.into_iter().next().unwrap() {
        ActorCommand::PublishSignedEvent {
            target,
            correlation_id,
            ..
        } => {
            assert_eq!(target, crate::publish::PublishTarget::Auto);
            assert_eq!(
                correlation_id,
                Some(minted_correlation_id),
                "the executor must thread the minted correlation_id onto the command"
            );
        }
        other => panic!("a pre-signed Publish must route to PublishSignedEvent, got {other:?}"),
    }
}

#[test]
fn start_publish_profile_action_with_string_fields_is_accepted() {
    // `PublishAction::PublishProfile` with a flat string-valued `fields`
    // map passes `PublishModule::start`'s validation gate — the
    // `ActionModule`-native path for kind:0 metadata publish. The
    // one-door-per-capability rule deleted the prior
    // `nmp_app_publish_unsigned_event` FFI symbol; this `nmp.publish`
    // dispatch is the sole entrypoint for it.
    let registry = default_registry();
    let action_json = r#"{"PublishProfile":{"fields":{"name":"Alice","about":"hello"}}}"#;
    let id = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", action_json)
        .expect("publish-profile action with string fields should be accepted");
    assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit()),
        "correlation id should be hex: {id}"
    );
}

#[test]
fn start_publish_profile_action_with_non_string_field_is_rejected() {
    // A kind:0 `content` is a flat JSON object of string values — a
    // numeric (or any non-string) field is rejected at `start`.
    let registry = default_registry();
    let action_json = r#"{"PublishProfile":{"fields":{"name":"Alice","age":42}}}"#;
    let err = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", action_json)
        .expect_err("non-string profile field must be rejected");
    match err {
        ActionRejection::Invalid(msg) => {
            assert!(msg.contains("must be a string value"), "got: {msg}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

/// The `nmp.publish` executor threads the registry-minted `correlation_id`
/// onto `ActorCommand::PublishProfile`. The actor signs the event, so its
/// id is unknown at dispatch time — without this the publish engine could
/// not report the host's correlation_id in `action_results`. Exercises
/// the real `default_registry()` executor closure via `execute()`.
#[test]
fn publish_profile_executor_threads_correlation_id_onto_actor_command() {
    use crate::actor::ActorCommand;
    use std::cell::RefCell;

    let registry = default_registry();
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());

    let minted_correlation_id = "ab".repeat(16);
    let action_json =
        r#"{"PublishProfile":{"fields":{"name":"Alice","picture":"https://x/y.png"}}}"#;
    registry
        .execute("nmp.publish", action_json, &minted_correlation_id, &|cmd| {
            captured.borrow_mut().push(cmd);
        })
        .expect("publish-profile execution should succeed");

    let cmds = captured.into_inner();
    assert_eq!(
        cmds.len(),
        1,
        "executor must emit exactly one ActorCommand; got {cmds:?}"
    );
    match cmds.into_iter().next().unwrap() {
        ActorCommand::PublishProfile {
            fields,
            correlation_id,
        } => {
            assert_eq!(
                fields.get("name").and_then(|v| v.as_str()),
                Some("Alice"),
                "the profile fields must be carried through verbatim"
            );
            assert_eq!(
                fields.get("picture").and_then(|v| v.as_str()),
                Some("https://x/y.png")
            );
            assert_eq!(
                correlation_id,
                Some(minted_correlation_id),
                "the executor must thread the minted correlation_id onto the command"
            );
        }
        other => panic!("expected ActorCommand::PublishProfile, got {other:?}"),
    }
}

#[test]
fn deliver_result_invokes_registered_observer() {
    use std::sync::{Arc, Mutex};
    // The observer captures every `ActionResult` it receives.
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
    // No observer registered — delivery must not panic.
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
        const NAMESPACE: &'static str = "host.boom_start"; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
        type Action = serde_json::Value;
        fn start(&self, _ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
            panic!("buggy module validator");
        }
        fn execute(
        &self,
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            Ok(())
        }
    }

    let mut registry = ActionRegistry::new();
    registry.register(PanickingStartModule);
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
    // A typed ActionModule whose execute() body panics. Its start() body
    // accepts every action shape (the panic must reach the executor, not
    // be caught at the validation gate).
    struct PanickingExecuteModule;
    impl ActionModule for PanickingExecuteModule {
        const NAMESPACE: &'static str = "host.boom"; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
        type Action = serde_json::Value;
        fn start(&self, _ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
            Ok(())
        }
        fn execute(
        &self,
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            panic!("buggy module executor");
        }
    }

    let mut registry = ActionRegistry::new();
    registry.register(PanickingExecuteModule);
    let err = registry
        .execute("host.boom", "null", "corr-id", &|_cmd| {})
        .expect_err("a panicking executor must return Err, not unwind");
    assert_eq!(err, "action executor panicked", "got: {err}");
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
        // Panic on the first call only — subsequent deliveries must
        // still reach the observer, proving panic isolation per-result.
        if n == 1 {
            panic!(
                "buggy host result observer (call #{}, corr={})",
                n, result.correlation_id
            );
        }
    });

    // First delivery: observer panics, `deliver_result` must NOT
    // propagate it (this test would abort the process if it did).
    registry.deliver_result(ActionResult {
        correlation_id: "first".to_string(),
        result_json: serde_json::Value::Null,
    });
    // Second delivery: observer is still live and receives the call.
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

// ---------------------------------------------------------------------------
// ADR-0049 Part 1 — directional registry semantics (order-independent yield)
// ---------------------------------------------------------------------------

mod adr_0049_yield {
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
}
