use super::*;

fn test_module() -> MarmotActionModule {
    use crate::projection::state::MarmotProjection;
    use crate::service::MarmotService;
    use mdk_core::MdkConfig;
    use mdk_sqlite_storage::MdkSqliteStorage;
    use nostr::Keys;

    let storage =
        MdkSqliteStorage::new_in_memory().expect("in-memory MDK storage should construct");
    let service = MarmotService::from_storage(storage, Keys::generate(), MdkConfig::default());
    let projection = Arc::new(MarmotProjection::new(service, None));
    MarmotActionModule::new(crate::runtime::MarmotRuntime::from_projection_for_tests(
        projection,
    ))
}

/// The typed enum's JSON shape MUST accept the supported host-produced
/// Marmot action bodies. The raw signed-event tap, not this action seam,
/// owns inbound event ingest.
#[test]
fn host_action_shapes_parse_as_typed_actions() {
    let cases = &[
        r#"{"op":"publish_key_package"}"#,
        r#"{"op":"create_group","name":"engineering","description":"the eng group","invitee_text":"npub1abc npub1def","signed_key_package_events_json":[]}"#,
        r#"{"op":"invite","group_id_hex":"aa00bb11","invitee_text":"npub1ghi","signed_key_package_events_json":[]}"#,
        r#"{"op":"send","group_id_hex":"aa00bb11","text":"hello"}"#,
        r#"{"op":"leave","group_id_hex":"aa00bb11"}"#,
        r#"{"op":"remove","group_id_hex":"aa00bb11","member_npubs":["npub1ghi"]}"#,
        r#"{"op":"accept_welcome","welcome_id_hex":"cc22dd33"}"#,
        r#"{"op":"decline_welcome","welcome_id_hex":"cc22dd33"}"#,
        r#"{"op":"clear_pending","group_id_hex":"aa00bb11"}"#,
    ];
    for json in cases {
        let parsed: MarmotAction = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("typed enum must accept host action `{json}`: {e}"));
        let reserialized = serde_json::to_string(&parsed).unwrap();
        let _: MarmotAction = serde_json::from_str(&reserialized).unwrap_or_else(|e| {
            panic!("re-serialized envelope must round-trip: {reserialized}: {e}")
        });
    }
}

/// The `op` discriminator MUST be snake_case — the same casing the iOS
/// bridge produces. A bug that flipped this to PascalCase would silently
/// break every iOS dispatch site after the migration.
#[test]
fn op_discriminator_is_snake_case() {
    let action = MarmotAction::PublishKeyPackage { relays: Vec::new() };
    let json = serde_json::to_string(&action).unwrap();
    assert!(
        json.contains(r#""op":"publish_key_package""#),
        "op discriminator must be snake_case, got: {json}"
    );
}

/// `MarmotActionModule::execute` MUST emit exactly one typed `Protocol`
/// command carrying the parsed action and registry-minted `correlation_id`.
#[test]
fn execute_emits_one_typed_protocol_command_with_correlation_id() {
    use nmp_core::actor::ActorCommand;
    use std::cell::RefCell;

    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    let action = MarmotAction::Send {
        group_id_hex: "aa00bb11".to_string(),
        text: "hello, group".to_string(),
    };
    test_module()
        .execute(
            &nmp_core::substrate::ActionContext::default(),
            action,
            "corr-test-id",
            &|cmd| {
                captured.borrow_mut().push(cmd);
            },
        )
        .expect("execute should not fail for a valid action");

    let cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1, "execute must emit exactly one ActorCommand");
    match cmds.into_iter().next().unwrap() {
        ActorCommand::Protocol(cmd) => {
            let dbg = format!("{cmd:?}");
            assert!(
                dbg.contains("MarmotProtocolCommand"),
                "expected a MarmotProtocolCommand, got: {dbg}"
            );
            assert!(
                dbg.contains("corr-test-id"),
                "must carry the registry-minted correlation_id, got: {dbg}"
            );
            assert!(
                dbg.contains("Send"),
                "must carry the typed action body, got: {dbg}"
            );
            assert!(
                dbg.contains("hello, group"),
                "must carry the action body, got: {dbg}"
            );
        }
        other => panic!("expected ActorCommand::Protocol, got {other:?}"),
    }
}

/// chirp#129 regression: a panic inside the `ops::dispatch` call MUST be
/// converted to a normal `{"ok":false,"error":...}` result, not propagate.
/// Without this, `MarmotProtocolCommand::run` never reaches its
/// `ctx.record_action_failure` call and the correlation_id's action stage
/// is stuck at `Requested` forever — the "Creating…" eternal hang.
#[test]
fn dispatch_with_panic_guard_converts_panic_to_ok_false() {
    // Silence the default panic hook for this one deliberate, expected panic
    // so the test output doesn't print a scary (but harmless) backtrace.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = super::dispatch_with_panic_guard(|| {
        panic!("simulated mdk-core/openmls invariant violation");
    });
    std::panic::set_hook(prev_hook);

    assert_eq!(
        result.get("ok").and_then(serde_json::Value::as_bool),
        Some(false),
        "a panic must surface as ok:false, not propagate: {result}"
    );
    let error = result
        .get("error")
        .and_then(serde_json::Value::as_str)
        .expect("error string present");
    assert!(
        error.contains("simulated mdk-core/openmls invariant violation"),
        "panic message must be preserved for diagnostics: {error}"
    );
}

/// The happy path: no panic means the guard is a transparent passthrough.
#[test]
fn dispatch_with_panic_guard_passes_through_normal_result() {
    let result = super::dispatch_with_panic_guard(|| serde_json::json!({"ok": true}));
    assert_eq!(result, serde_json::json!({"ok": true}));
}

/// Unknown `op` values fail at the registry's JSON-shape parse step.
#[test]
fn unknown_op_is_rejected_at_serde_layer() {
    let err = serde_json::from_str::<MarmotAction>(r#"{"op":"nuke_everything"}"#)
        .expect_err("unknown op must be rejected by serde");
    assert!(
        err.to_string().contains("unknown variant") || err.to_string().contains("nuke_everything"),
        "expected serde to name the offending variant, got: {err}"
    );
}
