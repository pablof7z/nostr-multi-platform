use super::*;
use std::cell::RefCell;

const EVENT_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AUTHOR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

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
fn highlight_with_all_fields_emits_expected_tags() {
    let action = PublishHighlightAction {
        content: "the highlighted text".to_string(),
        context: Some("surrounding context".to_string()),
        source_event_id: Some(EVENT_ID.to_string()),
        source_address: Some("30023:abc:my-article".to_string()),
        source_author_pubkey: Some(AUTHOR.to_string()),
        alt: Some("a highlight".to_string()),
        external_ids: vec![
            "podcast:item:guid:1234".to_string(),
            "https://example.com/article".to_string(),
        ],
        external_kinds: vec![
            "podcast:item:guid".to_string(),
            "web".to_string(),
            "podcast:item:guid".to_string(),
        ],
    };
    let cmd = capture_execute(|send| {
        PublishHighlightModule
            .execute(action, "hl-cid", send)
            .expect("execute succeeds");
    });
    match run_one_protocol(cmd) {
        ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id,
            signer_pubkey,
        }) => {
            assert_eq!(event.kind, KIND_HIGHLIGHT);
            assert_eq!(event.created_at, 0);
            assert_eq!(event.pubkey, "");
            assert_eq!(event.content, "the highlighted text");
            assert_eq!(
                event.tags,
                vec![
                    vec!["alt".to_string(), "a highlight".to_string()],
                    vec!["e".to_string(), EVENT_ID.to_string()],
                    vec!["a".to_string(), "30023:abc:my-article".to_string()],
                    vec!["p".to_string(), AUTHOR.to_string()],
                    vec!["context".to_string(), "surrounding context".to_string()],
                    vec!["i".to_string(), "podcast:item:guid:1234".to_string()],
                    vec!["i".to_string(), "https://example.com/article".to_string()],
                    vec!["k".to_string(), "podcast:item:guid".to_string()],
                    vec!["k".to_string(), "web".to_string()],
                ]
            );
            assert_eq!(correlation_id.as_deref(), Some("hl-cid"));
            assert_eq!(signer_pubkey, None);
        }
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}

#[test]
fn empty_content_is_rejected() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        PublishHighlightModule.start(
            &mut ctx,
            PublishHighlightAction {
                content: String::new(),
                context: None,
                source_event_id: None,
                source_address: None,
                source_author_pubkey: None,
                alt: None,
                external_ids: Vec::new(),
                external_kinds: Vec::new(),
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn invalid_source_event_id_is_rejected() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        PublishHighlightModule.start(
            &mut ctx,
            PublishHighlightAction {
                content: "text".to_string(),
                context: None,
                source_event_id: Some("not-hex".to_string()),
                source_address: None,
                source_author_pubkey: None,
                alt: None,
                external_ids: Vec::new(),
                external_kinds: Vec::new(),
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn external_only_highlight_emits_nip73_i_and_k_tags_with_no_attribution() {
    let mut ctx = ActionContext::default();
    let action = PublishHighlightAction {
        content: "clip text".to_string(),
        context: None,
        source_event_id: None,
        source_address: None,
        source_author_pubkey: None,
        alt: None,
        external_ids: vec!["podcast:item:guid:xyz".to_string()],
        external_kinds: vec!["podcast:item:guid".to_string()],
    };
    PublishHighlightModule
        .start(&mut ctx, action.clone())
        .expect("external-only highlight accepted");
    let cmd = capture_execute(|send| {
        PublishHighlightModule
            .execute(action, "ext-cid", send)
            .expect("execute succeeds");
    });
    match run_one_protocol(cmd) {
        ActorCommand::Publish(PublishCommand::UnsignedEvent { event, .. }) => {
            assert_eq!(event.kind, KIND_HIGHLIGHT);
            assert_eq!(
                event.tags,
                vec![
                    vec!["i".to_string(), "podcast:item:guid:xyz".to_string()],
                    vec!["k".to_string(), "podcast:item:guid".to_string()],
                ]
            );
        }
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}

#[test]
fn external_ids_require_external_kind() {
    let mut ctx = ActionContext::default();
    assert!(matches!(
        PublishHighlightModule.start(
            &mut ctx,
            PublishHighlightAction {
                content: "clip text".to_string(),
                context: None,
                source_event_id: None,
                source_address: None,
                source_author_pubkey: None,
                alt: None,
                external_ids: vec!["podcast:item:guid:xyz".to_string()],
                external_kinds: Vec::new(),
            },
        ),
        Err(ActionRejection::Invalid(_))
    ));
}
