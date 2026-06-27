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
    let (reg, tx) = empty_broker();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        &mut pending,
        &reg,
        &tx,
        &noop_wake(),
        &test_command_sender(),
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
    let (reg, tx) = empty_broker();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        &mut pending,
        &reg,
        &tx,
        &noop_wake(),
        &test_command_sender(),
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
    let (reg, tx) = empty_broker();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        &mut pending,
        &reg,
        &tx,
        &noop_wake(),
        &test_command_sender(),
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
    let (tx, _rx) = mpsc::channel::<SignerCompletion>();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        &mut pending,
        &reg,
        &tx,
        &noop_wake(),
        &test_command_sender(),
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
fn dispatch_bytes_nip17_send_drains_to_two_giftwrap_outbound_frames() {
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;
    use nmp_core::substrate::{DmInboxRelayLookup, TestDmInboxRelayCache};

    let (mut handle, sender_pubkey) = handle_with_local_key_signer();
    let recipient = LocalKeySigner::from_secret_hex(&"12".repeat(32)).expect("valid secret");
    let recipient_pubkey = recipient.pubkey().to_hex();

    let dm_relays = Arc::new(TestDmInboxRelayCache::new());
    dm_relays.upsert(&recipient_pubkey, &["wss://dm.example"]);
    dm_relays.upsert(&sender_pubkey, &["wss://dm.example"]);
    handle
        .runtime
        .reducer
        .set_dm_inbox_relay_lookup(dm_relays as Arc<dyn DmInboxRelayLookup>);

    let payload =
        send_dm_payload_bytes(&recipient_pubkey, "browser runtime sends a NIP-17 DM", None);
    let bytes = encode_dispatch_envelope("cid-nip17-send", "nmp.nip17.send", 1, &payload);

    let applied = handle.apply_dispatch_bytes(&bytes);
    assert!(
        matches!(applied, crate::runtime::DispatchBytesResult::Applied { .. }),
        "dispatch must accept the typed NIP-17 send bytes: {applied:?}"
    );

    let out = handle.pump();
    let giftwrap_events = out
        .outbound
        .iter()
        .filter(|msg| msg.text().starts_with("[\"EVENT\"") && msg.text().contains("\"kind\":1059"))
        .count();
    assert_eq!(
        giftwrap_events,
        2,
        "recipient and self-copy gift-wrap EVENT frames must be emitted: {:?}",
        out.outbound
            .iter()
            .map(|msg| msg.text())
            .collect::<Vec<_>>()
    );
}

#[test]
fn dispatch_bytes_nip17_send_uses_kind10050_ingested_from_relay_frames() {
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;
    use nmp_core::RelayFrame;
    use nmp_signer_iface::{SignerOp, UnsignedEvent};

    const RELAY: &str = "wss://dm.example";

    let (mut handle, _sender_pubkey) = handle_with_local_key_signer();
    let recipient = LocalKeySigner::from_secret_hex(&"12".repeat(32)).expect("valid secret");
    let recipient_pubkey = recipient.pubkey().to_hex();

    for frame in [
        relay_event_frame(
            "sender-10050",
            signed_kind10050_json(
                &LocalKeySigner::from_secret_hex(&"ee".repeat(32)).expect("valid secret"),
                RELAY,
            ),
        ),
        relay_event_frame("recipient-10050", signed_kind10050_json(&recipient, RELAY)),
    ] {
        let outbound = handle.runtime.reducer.handle_relay_frame(
            nmp_network::role::RelayRole::Indexer,
            RELAY,
            RelayFrame::Text(frame),
        );
        handle.fan_out_outbound(outbound);
    }

    let payload = send_dm_payload_bytes(
        &recipient_pubkey,
        "browser runtime sends with relay-ingested kind10050 state",
        None,
    );
    let bytes = encode_dispatch_envelope("cid-nip17-send", "nmp.nip17.send", 1, &payload);

    let applied = handle.apply_dispatch_bytes(&bytes);
    assert!(
        matches!(applied, crate::runtime::DispatchBytesResult::Applied { .. }),
        "dispatch must accept send after relay-ingested kind:10050 lists: {applied:?}"
    );

    let out = handle.pump();
    let giftwrap_events = out
        .outbound
        .iter()
        .filter(|msg| msg.text().starts_with("[\"EVENT\"") && msg.text().contains("\"kind\":1059"))
        .count();
    assert_eq!(
        giftwrap_events,
        2,
        "recipient and self-copy gift-wrap EVENT frames must be emitted from relay-ingested cache: {:?}",
        out.outbound
            .iter()
            .map(|msg| msg.text())
            .collect::<Vec<_>>()
    );

    fn signed_kind10050_json(signer: &LocalKeySigner, relay: &str) -> String {
        let unsigned = UnsignedEvent {
            pubkey: String::new(),
            kind: 10_050,
            tags: vec![vec!["relay".to_string(), relay.to_string()]],
            content: String::new(),
            created_at: 1,
        };
        match signer.sign(unsigned) {
            SignerOp::Ready(Ok(signed)) => signed.to_nip01_json(),
            SignerOp::Ready(Err(err)) => panic!("kind:10050 sign failed: {err}"),
            SignerOp::Pending(_) => panic!("local signer must complete synchronously"),
        }
    }

    fn relay_event_frame(sub_id: &str, event_json: String) -> String {
        format!(r#"["EVENT","{sub_id}",{event_json}]"#)
    }
}

fn send_dm_payload_bytes(recipient_pubkey: &str, content: &str, reply_to: Option<&str>) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};

    let mut fbb = FlatBufferBuilder::new();
    let recipient = fbb.create_string(recipient_pubkey);
    let content = fbb.create_string(content);
    let reply_to = reply_to.map(|value| fbb.create_string(value));
    let start = fbb.start_table();
    fbb.push_slot::<u32>(4 as VOffsetT, 1, 0);
    fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, recipient);
    fbb.push_slot_always::<WIPOffset<&str>>(8 as VOffsetT, content);
    if let Some(reply_to) = reply_to {
        fbb.push_slot_always::<WIPOffset<&str>>(10 as VOffsetT, reply_to);
    }
    let root = fbb.end_table(start);
    fbb.finish(root, Some("N17S"));
    fbb.finished_data().to_vec()
}

#[test]
fn unsupported_command_surfaces_command_failed() {
    let mut reducer = KernelReducer::new();
    let rx = enqueue(vec![ActorCommand::Lifecycle(LifecycleCommand::Stop)]);
    let mut pending = HashMap::new();
    let (reg, tx) = empty_broker();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        &mut pending,
        &reg,
        &tx,
        &noop_wake(),
        &test_command_sender(),
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
    let mut reg = CapabilityProviderRegistry::new();
    reg.insert(Arc::new(recipient) as Arc<dyn Signer>);
    let (_unused_reg, tx) = empty_broker();

    let out = drain_inbox(
        &mut reducer,
        &rx,
        &mut pending,
        &reg,
        &tx,
        &noop_wake(),
        &test_command_sender(),
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
    let (reg, tx) = empty_broker();

    let sender = test_command_sender();
    let first = drain_inbox(
        &mut reducer,
        &rx,
        &mut pending,
        &reg,
        &tx,
        &noop_wake(),
        &sender,
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
        &mut pending,
        &reg,
        &tx,
        &noop_wake(),
        &sender,
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
