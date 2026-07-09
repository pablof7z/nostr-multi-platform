use super::*;
use nmp_core::actor::PublishCommand;
use std::cell::RefCell;
use std::sync::Arc;

const ROOT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MID_THREAD_PARENT: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const MID_THREAD_PARENT_AUTHOR: &str =
    "3333333333333333333333333333333333333333333333333333333333333333";

fn capture_execute(action: ReplyAction) -> ActorCommand {
    capture_execute_with_ctx(&ActionContext::default(), action)
}

fn capture_execute_with_ctx(ctx: &ActionContext, action: ReplyAction) -> ActorCommand {
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    ReplyModule
        .execute(ctx, action, "reply-cid", &|cmd| {
            captured.borrow_mut().push(cmd)
        })
        .expect("execute succeeds");
    let mut cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1);
    cmds.pop().unwrap()
}

fn run_protocol(cmd: ActorCommand) -> ActorCommand {
    let ActorCommand::Protocol(cmd) = cmd else {
        panic!("expected protocol command");
    };
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    let send = |cmd| captured.borrow_mut().push(cmd);
    let mut ctx = ProtocolCommandContext::with_send_only(&send);
    cmd.run(&mut ctx).expect("protocol command runs");
    let mut cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1);
    cmds.pop().unwrap()
}

#[test]
fn event_action_publishes_nip22_for_non_note_target() {
    let cmd = capture_execute(ReplyAction {
        target_event_id: Some(ROOT.to_string()),
        target_kind: 30023,
        target_author_pubkey: Some(AUTHOR.to_string()),
        target_address: None,
        target_external_uri: None,
        relay_hint: None,
        content: "comment".to_string(),
    });

    match run_protocol(cmd) {
        ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id,
            signer_pubkey,
        }) => {
            assert_eq!(event.kind, KIND_NIP22_COMMENT);
            assert_eq!(event.pubkey, "");
            assert_eq!(event.created_at, 0);
            assert_eq!(correlation_id.as_deref(), Some("reply-cid"));
            assert_eq!(signer_pubkey, None);
            assert_eq!(event.tags[0], vec!["E", ROOT]);
        }
        other => panic!("expected publish command, got {other:?}"),
    }
}

#[test]
fn kind1_reply_to_a_mid_thread_uncached_parent_fails_closed_end_to_end() {
    // #3099 Bug A, end to end. A REAL local store is attached (not
    // `ActionContext::default()`, which is "no store at all") but does not
    // hold `MID_THREAD_PARENT` — the genuine bounded-cache-miss case the bug
    // was about: whether a reply's published root/reply tags depend on cache
    // luck. `MID_THREAD_PARENT` is (unknowably, from this scalar) a
    // mid-thread note; the old behavior silently tagged it as a fresh root.
    let ctx = ActionContext::with_event_store(Arc::new(nmp_store::MemEventStore::default()));
    let cmd = capture_execute_with_ctx(
        &ctx,
        ReplyAction {
            target_event_id: Some(MID_THREAD_PARENT.to_string()),
            target_kind: KIND_SHORT_TEXT_NOTE,
            target_author_pubkey: Some(MID_THREAD_PARENT_AUTHOR.to_string()),
            target_address: None,
            target_external_uri: None,
            relay_hint: None,
            content: "reply".to_string(),
        },
    );

    let ActorCommand::Protocol(protocol_cmd) = cmd else {
        panic!("expected protocol command");
    };
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    let send = |cmd| captured.borrow_mut().push(cmd);
    let mut proto_ctx = ProtocolCommandContext::with_send_only(&send);
    let err = protocol_cmd
        .run(&mut proto_ctx)
        .expect_err("must fail closed rather than fabricate a root marker");
    assert!(
        err.message().contains("parent"),
        "error should surface the parent-not-locally-known reason: {err}"
    );
    assert!(
        captured.into_inner().is_empty(),
        "no publish command may be emitted for a fabricated root"
    );
}

#[test]
fn start_rejects_ambiguous_targets() {
    let mut ctx = ActionContext::default();
    let err = ReplyModule
        .start(
            &mut ctx,
            ReplyAction {
                target_event_id: Some(ROOT.to_string()),
                target_kind: 1,
                target_author_pubkey: Some(AUTHOR.to_string()),
                target_address: Some("30023:pk:d".to_string()),
                target_external_uri: None,
                relay_hint: None,
                content: "x".to_string(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, ActionRejection::Invalid(_)));
}
