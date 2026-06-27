use super::*;
use nmp_kinds::KIND_NIP22_COMMENT;
use std::cell::RefCell;

const ROOT_EVENT: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const PARENT_COMMENT: &str = "4444444444444444444444444444444444444444444444444444444444444444";

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

fn published_event(action: PostCommentAction) -> UnsignedEvent {
    let cmd = capture_execute(|send| {
        PostCommentModule
            .execute(
                &nmp_core::substrate::ActionContext::default(),
                action,
                "comment-cid",
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
            assert_eq!(event.kind, KIND_NIP22_COMMENT);
            assert_eq!(event.created_at, 0);
            assert_eq!(event.pubkey, "");
            assert_eq!(correlation_id.as_deref(), Some("comment-cid"));
            assert_eq!(signer_pubkey, None);
            event
        }
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}

#[test]
fn top_level_comment_mirrors_root_scope() {
    let event = published_event(PostCommentAction {
        root_tag_name: "E".to_string(),
        root_tag_value: ROOT_EVENT.to_string(),
        root_kind: 11,
        parent_event_id: None,
        root_author_pubkey: None,
        parent_author_pubkey: None,
        content: "  hello thread  ".to_string(),
    });

    assert_eq!(event.content, "hello thread");
    assert_eq!(event.tags[0], vec!["E".to_string(), ROOT_EVENT.to_string()]);
    assert_eq!(event.tags[1], vec!["K".to_string(), "11".to_string()]);
    // Top-level parent mirrors the (lowercased) root.
    assert_eq!(event.tags[2], vec!["e".to_string(), ROOT_EVENT.to_string()]);
    assert_eq!(event.tags[3], vec!["k".to_string(), "11".to_string()]);
}

#[test]
fn reply_points_lowercase_parent_at_comment_kind_1111() {
    let event = published_event(PostCommentAction {
        root_tag_name: "E".to_string(),
        root_tag_value: ROOT_EVENT.to_string(),
        root_kind: 11,
        parent_event_id: Some(PARENT_COMMENT.to_string()),
        root_author_pubkey: None,
        parent_author_pubkey: None,
        content: "good point".to_string(),
    });

    // Root scope is unchanged; parent scope points at the parent comment.
    assert_eq!(event.tags[0], vec!["E".to_string(), ROOT_EVENT.to_string()]);
    assert_eq!(event.tags[1], vec!["K".to_string(), "11".to_string()]);
    assert_eq!(
        event.tags[2],
        vec!["e".to_string(), PARENT_COMMENT.to_string()]
    );
    assert_eq!(event.tags[3], vec!["k".to_string(), "1111".to_string()]);
}

#[test]
fn addressable_root_lowercases_scope_for_parent() {
    let address = "30023:pubkey:essay";
    let event = published_event(PostCommentAction {
        root_tag_name: "a".to_string(), // case is normalised to uppercase A
        root_tag_value: address.to_string(),
        root_kind: 30023,
        parent_event_id: None,
        root_author_pubkey: None,
        parent_author_pubkey: None,
        content: "nice essay".to_string(),
    });

    assert_eq!(event.tags[0], vec!["A".to_string(), address.to_string()]);
    assert_eq!(event.tags[1], vec!["K".to_string(), "30023".to_string()]);
    assert_eq!(event.tags[2], vec!["a".to_string(), address.to_string()]);
    assert_eq!(event.tags[3], vec!["k".to_string(), "30023".to_string()]);
}

#[test]
fn external_root_allows_non_hex_value() {
    let identifier = "podcast:item:guid:abc-123";
    let event = published_event(PostCommentAction {
        root_tag_name: "I".to_string(),
        root_tag_value: identifier.to_string(),
        root_kind: 0,
        parent_event_id: None,
        root_author_pubkey: None,
        parent_author_pubkey: None,
        content: "loved it".to_string(),
    });

    assert_eq!(event.tags[0], vec!["I".to_string(), identifier.to_string()]);
    assert_eq!(event.tags[2], vec!["i".to_string(), identifier.to_string()]);
}

#[test]
fn emits_root_and_parent_author_notify_tags_when_known() {
    let root_author = "5".repeat(64);
    let parent_author = "6".repeat(64);
    let event = published_event(PostCommentAction {
        root_tag_name: "E".to_string(),
        root_tag_value: ROOT_EVENT.to_string(),
        root_kind: 11,
        parent_event_id: Some(PARENT_COMMENT.to_string()),
        root_author_pubkey: Some(root_author.clone()),
        parent_author_pubkey: Some(parent_author.clone()),
        content: "ping both".to_string(),
    });

    // Uppercase `P` (root author) rides the root scope; lowercase `p` (parent
    // author) rides the parent scope.
    assert!(event.tags.contains(&vec!["P".to_string(), root_author]));
    assert!(event.tags.contains(&vec!["p".to_string(), parent_author]));
}

#[test]
fn omits_author_tags_when_unknown() {
    let event = published_event(PostCommentAction {
        root_tag_name: "E".to_string(),
        root_tag_value: ROOT_EVENT.to_string(),
        root_kind: 11,
        parent_event_id: None,
        root_author_pubkey: None,
        parent_author_pubkey: None,
        content: "no pings".to_string(),
    });

    assert!(
        !event
            .tags
            .iter()
            .any(|tag| tag.first().map(String::as_str) == Some("P"))
    );
    assert!(
        !event
            .tags
            .iter()
            .any(|tag| tag.first().map(String::as_str) == Some("p"))
    );
}

#[test]
fn rejects_non_hex_root_author() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        PostCommentModule.start(
            &mut ctx,
            PostCommentAction {
                root_tag_name: "E".to_string(),
                root_tag_value: ROOT_EVENT.to_string(),
                root_kind: 11,
                parent_event_id: None,
                root_author_pubkey: Some("nope".to_string()),
                parent_author_pubkey: None,
                content: "hi".to_string(),
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn rejects_blank_content() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        PostCommentModule.start(
            &mut ctx,
            PostCommentAction {
                root_tag_name: "E".to_string(),
                root_tag_value: ROOT_EVENT.to_string(),
                root_kind: 11,
                parent_event_id: None,
                root_author_pubkey: None,
                parent_author_pubkey: None,
                content: "   ".to_string(),
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn rejects_non_hex_event_root() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        PostCommentModule.start(
            &mut ctx,
            PostCommentAction {
                root_tag_name: "E".to_string(),
                root_tag_value: "not-hex".to_string(),
                root_kind: 11,
                parent_event_id: None,
                root_author_pubkey: None,
                parent_author_pubkey: None,
                content: "hi".to_string(),
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn rejects_unknown_root_scope() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        PostCommentModule.start(
            &mut ctx,
            PostCommentAction {
                root_tag_name: "X".to_string(),
                root_tag_value: ROOT_EVENT.to_string(),
                root_kind: 11,
                parent_event_id: None,
                root_author_pubkey: None,
                parent_author_pubkey: None,
                content: "hi".to_string(),
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn rejects_non_hex_parent() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        PostCommentModule.start(
            &mut ctx,
            PostCommentAction {
                root_tag_name: "E".to_string(),
                root_tag_value: ROOT_EVENT.to_string(),
                root_kind: 11,
                parent_event_id: Some("bad".to_string()),
                root_author_pubkey: None,
                parent_author_pubkey: None,
                content: "hi".to_string(),
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}
