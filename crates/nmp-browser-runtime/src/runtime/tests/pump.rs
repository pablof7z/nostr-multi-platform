use super::*;

#[derive(Debug)]
struct ReactionProtocolCommand;

impl nmp_core::substrate::ProtocolCommand for ReactionProtocolCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut nmp_core::substrate::ProtocolCommandContext<'_>,
    ) -> Result<(), nmp_core::substrate::ProtocolCommandError> {
        ctx.publish_unsigned(
            UnsignedEvent {
                pubkey: String::new(),
                kind: 7,
                tags: vec![vec!["e".to_string(), "aa".repeat(32)]],
                content: "+".to_string(),
                created_at: 0,
            },
            Some("react-protocol-cid".to_string()),
            None,
        );
        Ok(())
    }
}

#[test]
fn applied_command_produces_no_events_and_no_pending() {
    let mut reducer = KernelReducer::new();
    let rx = enqueue(vec![ActorCommand::Lifecycle(
        LifecycleCommand::MarkChangedSinceEmit,
    )]);
    let mut pending = HashMap::new();
    let mut pending_signs = PendingSignerCompletions::new();
    let mut pending_ciphers = PendingCipherCompletions::new();
    let (reg, tx) = empty_broker();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        drain_context(
            &mut pending,
            &reg,
            &mut pending_signs,
            &mut pending_ciphers,
            &tx,
            &noop_wake(),
            &test_command_sender(),
        ),
    );

    assert!(out.events.is_empty(), "Applied must emit no host event");
    assert!(!out.yielded, "single command must not hit the drain budget");
    assert!(pending.is_empty(), "Applied must not park a sign request");
}

#[test]
fn protocol_command_expands_before_headless_interpretation() {
    let mut reducer = KernelReducer::new();
    reducer.set_active_account_for_test("ab".repeat(32));
    let rx = enqueue(vec![ActorCommand::Protocol(Box::new(
        ReactionProtocolCommand,
    ))]);
    let mut pending = HashMap::new();
    let mut pending_signs = PendingSignerCompletions::new();
    let mut pending_ciphers = PendingCipherCompletions::new();
    let (reg, tx) = empty_broker();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        drain_context(
            &mut pending,
            &reg,
            &mut pending_signs,
            &mut pending_ciphers,
            &tx,
            &noop_wake(),
            &test_command_sender(),
        ),
    );

    assert_eq!(
        out.events.len(),
        1,
        "expanded unsigned publish must request a signature"
    );
    let BrowserRuntimeEvent::SignRequest { unsigned_json, .. } = &out.events[0] else {
        panic!("expected SignRequest, got {:?}", out.events[0]);
    };
    assert!(
        unsigned_json.contains("\"kind\":7"),
        "reaction protocol command must become unsigned kind:7 json: {unsigned_json}"
    );
    assert_eq!(pending.len(), 1, "sign continuation must be parked");
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
    let mut pending_signs = PendingSignerCompletions::new();
    let mut pending_ciphers = PendingCipherCompletions::new();
    let (reg, tx) = empty_broker();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        drain_context(
            &mut pending,
            &reg,
            &mut pending_signs,
            &mut pending_ciphers,
            &tx,
            &noop_wake(),
            &test_command_sender(),
        ),
    );

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
fn local_event_for_account_invokes_continuation_inline() {
    let signer = LocalKeySigner::generate();
    let pubkey = signer.pubkey().to_hex();
    let mut reg = CapabilityProviderRegistry::new();
    reg.insert(Arc::new(signer) as Arc<dyn Signer>);

    let signed_pubkey = Arc::new(Mutex::new(None::<String>));
    let signed_pubkey_for_continuation = Arc::clone(&signed_pubkey);
    let unsigned = UnsignedEvent {
        pubkey: pubkey.clone(),
        kind: 1,
        tags: Vec::new(),
        content: "browser sign port".to_string(),
        created_at: 1,
    };
    let cmd = ActorCommand::Sign(SignCommand::EventForAccount {
        unsigned,
        signer_pubkey: Some(pubkey.clone()),
        continuation: nmp_core::actor::SignContinuation::new(move |outcome| {
            *signed_pubkey_for_continuation.lock().expect("capture lock") =
                Some(outcome.expect("local sign must succeed").unsigned.pubkey);
        }),
    });
    let rx = enqueue(vec![cmd]);
    let mut reducer = KernelReducer::new();
    reducer.set_active_account_for_test(pubkey.clone());
    let mut pending = HashMap::new();
    let mut pending_signs = PendingSignerCompletions::new();
    let mut pending_ciphers = PendingCipherCompletions::new();
    let (tx, _rx) = mpsc::channel::<SignerCompletion>();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        drain_context(
            &mut pending,
            &reg,
            &mut pending_signs,
            &mut pending_ciphers,
            &tx,
            &noop_wake(),
            &test_command_sender(),
        ),
    );

    assert!(
        out.events.is_empty(),
        "inline sign port should not emit host events"
    );
    assert!(
        pending.is_empty(),
        "generic sign port is not a publish continuation"
    );
    assert_eq!(
        signed_pubkey.lock().expect("capture lock").as_deref(),
        Some(pubkey.as_str()),
        "continuation must receive the signed event"
    );
}

#[test]
fn unsupported_command_surfaces_command_failed() {
    let mut reducer = KernelReducer::new();
    let rx = enqueue(vec![ActorCommand::Lifecycle(LifecycleCommand::Stop)]);
    let mut pending = HashMap::new();
    let mut pending_signs = PendingSignerCompletions::new();
    let mut pending_ciphers = PendingCipherCompletions::new();
    let (reg, tx) = empty_broker();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        drain_context(
            &mut pending,
            &reg,
            &mut pending_signs,
            &mut pending_ciphers,
            &tx,
            &noop_wake(),
            &test_command_sender(),
        ),
    );

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
fn nip44_decrypt_for_account_resolves_local_key_continuation() {
    let recipient = LocalKeySigner::from_secret_hex(&"22".repeat(32)).expect("valid secret");
    let recipient_pubkey = recipient.pubkey();
    let recipient_hex = recipient_pubkey.to_hex();
    let peer = nostr::Keys::generate();
    let plaintext = "browser runtime decrypts the DM";
    let ciphertext = nostr::nips::nip44::encrypt(
        peer.secret_key(),
        &recipient_pubkey,
        plaintext,
        nostr::nips::nip44::Version::V2,
    )
    .expect("fixture encrypt");

    let captured = Arc::new(Mutex::new(None));
    let captured_for_continuation = Arc::clone(&captured);
    let continuation = CipherContinuation::new(move |outcome| {
        *captured_for_continuation.lock().expect("capture lock") = Some(outcome);
    });

    let mut reducer = KernelReducer::new();
    let _ = reducer.set_active_account(recipient_hex.clone());
    let rx = enqueue(vec![ActorCommand::Sign(
        SignCommand::Nip44DecryptForAccount {
            peer_pubkey: peer.public_key().to_hex(),
            ciphertext,
            signer_pubkey: None,
            continuation,
        },
    )]);
    let mut pending = HashMap::new();
    let mut pending_signs = PendingSignerCompletions::new();
    let mut pending_ciphers = PendingCipherCompletions::new();
    let mut reg = CapabilityProviderRegistry::new();
    reg.insert(Arc::new(recipient) as Arc<dyn Signer>);
    let (_unused_reg, tx) = empty_broker();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        drain_context(
            &mut pending,
            &reg,
            &mut pending_signs,
            &mut pending_ciphers,
            &tx,
            &noop_wake(),
            &test_command_sender(),
        ),
    );

    assert!(
        out.events.is_empty(),
        "decrypt is delivered by continuation"
    );
    assert!(
        pending.is_empty(),
        "cipher continuations are not publish signs"
    );
    assert_eq!(
        captured
            .lock()
            .expect("capture lock")
            .take()
            .expect("continuation ran")
            .expect("decrypt succeeded"),
        plaintext
    );
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
    let mut pending_signs = PendingSignerCompletions::new();
    let mut pending_ciphers = PendingCipherCompletions::new();
    let (reg, tx) = empty_broker();

    let sender = test_command_sender();
    let first = drain_inbox(
        &mut reducer,
        &rx,
        drain_context(
            &mut pending,
            &reg,
            &mut pending_signs,
            &mut pending_ciphers,
            &tx,
            &noop_wake(),
            &sender,
        ),
    );
    assert_eq!(
        first.events.len(),
        BROWSER_COMMAND_DRAIN_BUDGET,
        "first pump applies exactly the budget"
    );
    assert!(first.yielded, "budget hit must signal a re-pump");

    let second = drain_inbox(
        &mut reducer,
        &rx,
        drain_context(
            &mut pending,
            &reg,
            &mut pending_signs,
            &mut pending_ciphers,
            &tx,
            &noop_wake(),
            &sender,
        ),
    );
    assert_eq!(second.events.len(), 10, "remainder drains on the next pump");
    assert!(
        !second.yielded,
        "remainder is under budget - no further yield"
    );
}

#[test]
fn yielded_pump_fires_wake_for_followup_turn() {
    let mut handle = started_handle();
    let wake_count = install_counting_wake(&mut handle);
    let sender = handle.command_sender();
    for _ in 0..(BROWSER_COMMAND_DRAIN_BUDGET + 1) {
        sender
            .send(ActorCommand::Lifecycle(LifecycleCommand::Stop))
            .expect("send through command inbox");
    }

    let first = handle.pump();
    assert!(first.yielded, "budget hit must yield");
    assert_eq!(
        wake_count.get(),
        1,
        "yielded pump must schedule the next bounded turn"
    );

    let second = handle.pump();
    assert!(!second.yielded, "second turn drains the remainder");
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
