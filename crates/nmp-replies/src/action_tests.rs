use super::*;
use nmp_core::actor::PublishCommand;
use std::cell::RefCell;

const ROOT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn capture_execute(action: ReplyAction) -> ActorCommand {
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    ReplyModule
        .execute(&ActionContext::default(), action, "reply-cid", &|cmd| {
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
