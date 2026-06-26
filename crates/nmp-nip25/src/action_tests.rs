use super::*;
use std::cell::RefCell;

const TARGET: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AUTHOR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const REACTION: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn run_one_protocol(cmd: ActorCommand) -> ActorCommand {
    let ActorCommand::Protocol(cmd) = cmd else {
        panic!("expected protocol command, got {cmd:?}");
    };
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    let send = |cmd| captured.borrow_mut().push(cmd);
    let mut ctx = ProtocolCommandContext::with_send_only(&send);
    cmd.run(&mut ctx).expect("protocol command runs");
    let mut cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1, "expected one follow-up command");
    cmds.pop().unwrap()
}

fn capture_execute(run: impl FnOnce(&dyn Fn(ActorCommand))) -> ActorCommand {
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    run(&|cmd| captured.borrow_mut().push(cmd));
    let mut cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1, "expected one command");
    cmds.pop().unwrap()
}

#[test]
fn react_defaults_content_to_plus() {
    let action: ReactAction = serde_json::from_str(
        r#"{"target_event_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
    )
    .expect("valid JSON");
    assert_eq!(action.reaction, "+");
}

#[test]
fn react_rejects_malformed_ids() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        ReactModule.start(
            &mut ctx,
            ReactAction {
                target_event_id: "bad".to_string(),
                reaction: "+".to_string(),
                target_author_pubkey: None,
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn react_protocol_publishes_kind7_via_one_door() {
    let cmd = capture_execute(|send| {
        ReactModule
            .execute(
                &nmp_core::substrate::ActionContext::default(),
                ReactAction {
                    target_event_id: TARGET.to_string(),
                    reaction: String::new(),
                    target_author_pubkey: Some(AUTHOR.to_string()),
                },
                "react-cid",
                send,
            )
            .expect("execute succeeds");
    });
    match run_one_protocol(cmd) {
        ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id,
            signer_pubkey,
        }) => {
            assert_eq!(event.kind, KIND_REACTION);
            assert_eq!(event.created_at, 0);
            assert_eq!(event.pubkey, "");
            assert_eq!(event.content, "+");
            assert_eq!(event.tags[0], vec!["e".to_string(), TARGET.to_string()]);
            assert_eq!(event.tags[1], vec!["p".to_string(), AUTHOR.to_string()]);
            assert_eq!(correlation_id.as_deref(), Some("react-cid"));
            assert_eq!(signer_pubkey, None);
        }
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}

#[test]
fn unreact_protocol_publishes_kind5_deletion() {
    let cmd = capture_execute(|send| {
        UnreactModule
            .execute(
                &nmp_core::substrate::ActionContext::default(),
                UnreactAction {
                    reaction_event_id: REACTION.to_string(),
                    reason: "undo".to_string(),
                },
                "unreact-cid",
                send,
            )
            .expect("execute succeeds");
    });
    match run_one_protocol(cmd) {
        ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id,
            signer_pubkey,
        }) => {
            assert_eq!(event.kind, KIND_REACTION_DELETE);
            assert_eq!(
                event.tags,
                vec![vec!["e".to_string(), REACTION.to_string()]]
            );
            assert_eq!(event.content, "undo");
            assert_eq!(correlation_id.as_deref(), Some("unreact-cid"));
            assert_eq!(signer_pubkey, None);
        }
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}
