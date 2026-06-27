use super::*;
use std::cell::RefCell;

const TARGET_EVENT: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const TARGET_AUTHOR: &str = "4444444444444444444444444444444444444444444444444444444444444444";

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

fn capture_execute(action: RepostAction) -> ActorCommand {
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    RepostModule
        .execute(&ActionContext::default(), action, "repost-cid", &|cmd| {
            captured.borrow_mut().push(cmd)
        })
        .expect("execute succeeds");
    let mut cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1, "expected one command");
    cmds.pop().unwrap()
}

fn published_event(action: RepostAction) -> UnsignedEvent {
    match run_one_protocol(capture_execute(action)) {
        ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id,
            signer_pubkey,
        }) => {
            assert_eq!(event.created_at, 0);
            assert_eq!(event.pubkey, "");
            assert_eq!(correlation_id.as_deref(), Some("repost-cid"));
            assert_eq!(signer_pubkey, None);
            event
        }
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}

#[test]
fn kind_one_target_publishes_kind_six_repost() {
    let event = published_event(RepostAction {
        target_event_id: TARGET_EVENT.to_string(),
        target_kind: 1,
        target_author_pubkey: Some(TARGET_AUTHOR.to_string()),
        relay_hint: Some("wss://relay.example".to_string()),
    });

    assert_eq!(event.kind, KIND_REPOST);
    assert_eq!(
        event.tags,
        vec![
            vec![
                "e".to_string(),
                TARGET_EVENT.to_string(),
                "wss://relay.example".to_string()
            ],
            vec!["p".to_string(), TARGET_AUTHOR.to_string()],
            vec!["k".to_string(), "1".to_string()]
        ]
    );
    assert_eq!(event.content, "");
}

#[test]
fn non_kind_one_target_publishes_generic_repost() {
    let event = published_event(RepostAction {
        target_event_id: TARGET_EVENT.to_string(),
        target_kind: 30023,
        target_author_pubkey: None,
        relay_hint: None,
    });

    assert_eq!(event.kind, KIND_GENERIC_REPOST);
    assert_eq!(
        event.tags,
        vec![
            vec!["e".to_string(), TARGET_EVENT.to_string()],
            vec!["k".to_string(), "30023".to_string()]
        ]
    );
}

#[test]
fn rejects_malformed_target_event_id() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        RepostModule.start(
            &mut ctx,
            RepostAction {
                target_event_id: "not-hex".to_string(),
                target_kind: 1,
                target_author_pubkey: None,
                relay_hint: None,
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn rejects_zero_target_kind() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        RepostModule.start(
            &mut ctx,
            RepostAction {
                target_event_id: TARGET_EVENT.to_string(),
                target_kind: 0,
                target_author_pubkey: None,
                relay_hint: None,
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn rejects_malformed_target_author() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        RepostModule.start(
            &mut ctx,
            RepostAction {
                target_event_id: TARGET_EVENT.to_string(),
                target_kind: 1,
                target_author_pubkey: Some("not-hex".to_string()),
                relay_hint: None,
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}
