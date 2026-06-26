use super::*;

#[test]
fn applied_command_produces_no_events_and_no_pending() {
    let mut reducer = KernelReducer::new();
    let rx = enqueue(vec![ActorCommand::Lifecycle(
        LifecycleCommand::MarkChangedSinceEmit,
    )]);
    let mut pending = HashMap::new();
    let (reg, tx) = empty_broker();

    let out = drain_inbox(&mut reducer, &rx, &mut pending, &reg, &tx, &noop_wake());

    assert!(out.events.is_empty(), "Applied must emit no host event");
    assert!(!out.yielded, "single command must not hit the drain budget");
    assert!(pending.is_empty(), "Applied must not park a sign request");
}

#[test]
fn needs_sign_parks_continuation_and_emits_sign_request() {
    let mut reducer = KernelReducer::new();
    reducer.set_active_account_for_test("ab".repeat(32));

    let cmd = ActorCommand::Publish(PublishCommand::Profile {
        fields: serde_json::Map::new(),
        correlation_id: Some("cid-profile".to_string()),
    });
    let rx = enqueue(vec![cmd]);
    let mut pending = HashMap::new();
    let (reg, tx) = empty_broker();

    let out = drain_inbox(&mut reducer, &rx, &mut pending, &reg, &tx, &noop_wake());

    assert_eq!(out.events.len(), 1, "exactly one SignRequest expected");
    let BrowserRuntimeEvent::SignRequest {
        account_pubkey,
        unsigned_json,
        ..
    } = &out.events[0]
    else {
        panic!("expected SignRequest, got {:?}", out.events[0]);
    };
    assert_eq!(account_pubkey, &"ab".repeat(32));
    assert!(
        unsigned_json.contains("\"kind\":0"),
        "unsigned profile json must carry kind:0"
    );
    assert_eq!(pending.len(), 1, "publish continuation must be parked");
}

#[test]
fn unsupported_command_surfaces_command_failed() {
    let mut reducer = KernelReducer::new();
    let rx = enqueue(vec![ActorCommand::Lifecycle(LifecycleCommand::Stop)]);
    let mut pending = HashMap::new();
    let (reg, tx) = empty_broker();

    let out = drain_inbox(&mut reducer, &rx, &mut pending, &reg, &tx, &noop_wake());

    assert_eq!(out.events.len(), 1, "Unsupported must surface one failure");
    let BrowserRuntimeEvent::CommandFailed { reason } = &out.events[0] else {
        panic!("expected CommandFailed, got {:?}", out.events[0]);
    };
    assert!(
        reason.contains("browser_command_unsupported"),
        "failure reason must name the headless-unsupported discriminant: {reason}"
    );
    assert!(pending.is_empty());
}

#[test]
fn drain_is_bounded_by_budget_and_remainder_drains_next_pump() {
    let mut reducer = KernelReducer::new();
    let total = BROWSER_COMMAND_DRAIN_BUDGET + 10;
    let cmds: Vec<ActorCommand> = (0..total)
        .map(|_| ActorCommand::Lifecycle(LifecycleCommand::Stop))
        .collect();
    let rx = enqueue(cmds);
    let mut pending = HashMap::new();
    let (reg, tx) = empty_broker();

    let first = drain_inbox(&mut reducer, &rx, &mut pending, &reg, &tx, &noop_wake());
    assert_eq!(
        first.events.len(),
        BROWSER_COMMAND_DRAIN_BUDGET,
        "first pump applies exactly the budget"
    );
    assert!(first.yielded, "budget hit must signal a re-pump");

    let second = drain_inbox(&mut reducer, &rx, &mut pending, &reg, &tx, &noop_wake());
    assert_eq!(second.events.len(), 10, "remainder drains on the next pump");
    assert!(
        !second.yielded,
        "remainder is under budget - no further yield"
    );
}

#[test]
fn start_registers_defaults_and_pumps_clean() {
    let mut handle = started_handle();

    let out = handle.pump();
    assert!(out.outbound.is_empty());
    assert!(out.events.is_empty());
    assert!(!out.yielded);
    assert_eq!(handle.pending_sign_count(), 0);

    let frame = handle.make_update_frame(true);
    assert!(
        !frame.is_empty(),
        "update frame must be non-empty after start"
    );
}

#[test]
fn command_sender_round_trips_through_pump() {
    let mut handle = started_handle();
    let sender = handle.command_sender();
    sender
        .send(ActorCommand::Lifecycle(LifecycleCommand::Stop))
        .expect("send through command inbox");

    let out = handle.pump();
    assert_eq!(out.events.len(), 1);
    assert!(matches!(
        out.events[0],
        BrowserRuntimeEvent::CommandFailed { .. }
    ));
}

#[test]
fn configured_relays_snapshot_is_empty_after_without_initial_relays() {
    let handle = started_handle();
    assert!(
        handle.configured_relays().as_slice().is_empty(),
        "without_initial_relays must start with no configured relays"
    );
}
