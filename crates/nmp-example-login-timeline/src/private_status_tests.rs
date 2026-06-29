use super::*;
use nmp_core::dispatch_envelope::{
    decode_dispatch_envelope, encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION,
};
use nmp_core::substrate::{
    ActionRegistrar, NoopActionStageTracker, NoopErrorSurface, NoopHostOpHandlerAccess,
    NoopKernelClock, NoopLocalSignerAccess, NoopRecipientRelayLookup, NoopWalletKernelAccess,
    NoopZapProfileLookup, ProtocolCommandContextParts,
};
use nmp_core::ActionRegistry;
use std::cell::RefCell;

fn sample_action() -> PublishStatusAction {
    PublishStatusAction {
        title: "Launch Notes".to_string(),
        body: "The starter app owns this private kind.".to_string(),
        topics: vec!["starter".to_string(), "appkind".to_string()],
    }
}

fn generated_builder_shaped_bytes(correlation_id: &str, action: &PublishStatusAction) -> Vec<u8> {
    encode_dispatch_envelope(
        correlation_id,
        ACTION_NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &action.encode(),
    )
}

fn capture_execute(run: impl FnOnce(&dyn Fn(ActorCommand))) -> ActorCommand {
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    run(&|cmd| captured.borrow_mut().push(cmd));
    let mut cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1, "expected one command");
    cmds.pop().unwrap()
}

fn run_one_protocol(cmd: ActorCommand) -> ActorCommand {
    let ActorCommand::Protocol(cmd) = cmd else {
        panic!("expected protocol command, got {cmd:?}");
    };
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    let send = |cmd| captured.borrow_mut().push(cmd);
    let (command_sender, _rx) = std::sync::mpsc::channel::<nmp_core::actor::ActorMail>();
    let command_sender = nmp_core::CommandSender::new(command_sender);
    let mut ctx = ProtocolCommandContext::new(ProtocolCommandContextParts {
        send: &send,
        command_sender,
        clock: &NoopKernelClock,
        signers: &NoopLocalSignerAccess,
        dms: &nmp_core::substrate::EmptyDmInboxRelayLookup,
        errors: &NoopErrorSurface,
        stages: &NoopActionStageTracker,
        recipients: &NoopRecipientRelayLookup,
        host_op_handler: &NoopHostOpHandlerAccess,
        wallet_kernel: &NoopWalletKernelAccess,
        zap_profiles: &NoopZapProfileLookup,
    });
    cmd.run(&mut ctx).expect("protocol command runs");
    let mut cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1, "expected one follow-up command");
    cmds.pop().unwrap()
}

#[test]
fn generated_builder_envelope_round_trips_through_app_owned_payload_decode() {
    let action = sample_action();
    let bytes = generated_builder_shaped_bytes("app-kind-cid", &action);
    let decoded = decode_dispatch_envelope(&bytes).expect("dispatch envelope decodes");

    assert_eq!(decoded.correlation_id, "app-kind-cid");
    assert_eq!(decoded.action_namespace, ACTION_NAMESPACE);

    let payload =
        PublishStatusAction::decode(&decoded.payload).expect("app-owned ActionPayload decodes");
    assert_eq!(payload, action);
}

#[test]
fn explicit_composition_registers_app_private_action_namespace() {
    let mut registry = ActionRegistry::new();
    ActionRegistrar::register_action(&mut registry, PublishStatusModule)
        .expect("app-private action registers explicitly");
    let mut ctx = ActionContext::default();
    let correlation_id = registry
        .start_bytes(&mut ctx, 42, ACTION_NAMESPACE, &sample_action().encode())
        .expect("registered app-private action accepts typed bytes");
    assert_eq!(correlation_id, "000000000000002a0000000000000000");
}

#[test]
fn execution_publishes_declared_app_private_event_kind() {
    let action = sample_action();
    let cmd = capture_execute(|send| {
        PublishStatusModule
            .execute(&ActionContext::default(), action, "private-kind-cid", send)
            .expect("execute succeeds");
    });

    match run_one_protocol(cmd) {
        ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id,
            signer_pubkey,
        }) => {
            assert_eq!(event.kind, EVENT_KIND);
            assert_eq!(event.kind, 30444);
            assert_ne!(event.kind, 1, "must not collapse to a built-in note kind");
            assert_eq!(event.content, "The starter app owns this private kind.");
            assert_eq!(
                event.tags[0],
                vec!["d".to_string(), "launch-notes".to_string()]
            );
            assert_eq!(
                event.tags[1],
                vec!["alt".to_string(), "Launch Notes".to_string()]
            );
            assert_eq!(event.tags[2], vec!["t".to_string(), "starter".to_string()]);
            assert_eq!(event.tags[3], vec!["t".to_string(), "appkind".to_string()]);
            assert_eq!(correlation_id.as_deref(), Some("private-kind-cid"));
            assert_eq!(signer_pubkey, None);
        }
        other => panic!("expected app private UnsignedEvent publish, got {other:?}"),
    }
}

#[test]
fn payload_decode_rejects_schema_version_drift() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let title = fbb.create_string("status");
    let body = fbb.create_string("body");
    let payload = status_fb::PublishStatusPayload::create(
        &mut fbb,
        &status_fb::PublishStatusPayloadArgs {
            schema_version: SCHEMA_VERSION + 1,
            title: Some(title),
            body: Some(body),
            topics: None,
        },
    );
    status_fb::finish_publish_status_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();

    assert!(matches!(
        PublishStatusAction::decode(&bytes),
        Err(ActionPayloadDecodeError::SchemaVersionMismatch { expected: 1, .. })
    ));
}
