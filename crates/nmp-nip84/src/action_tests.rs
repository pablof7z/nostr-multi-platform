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
    };
    let cmd = capture_execute(|send| {
        PublishHighlightModule
            .execute(
                &nmp_core::substrate::ActionContext::default(),
                action,
                "hl-cid",
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
    };
    PublishHighlightModule
        .start(&mut ctx, action.clone())
        .expect("external-only highlight accepted");
    let cmd = capture_execute(|send| {
        PublishHighlightModule
            .execute(
                &nmp_core::substrate::ActionContext::default(),
                action,
                "ext-cid",
                send,
            )
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
fn blockchain_external_ids_derive_chain_selector_kind() {
    let action = PublishHighlightAction {
        content: "chain reference".to_string(),
        context: None,
        source_event_id: None,
        source_address: None,
        source_author_pubkey: None,
        alt: None,
        external_ids: vec![
            "bitcoin:tx:a1075db55d416d3ca199f55b6084e2115b9345e16c5cf302fc80e9d5fbf5d48d"
                .to_string(),
            "ethereum:1:address:0xd8da6bf26964af9d7eed9e03e53415d37aa96045".to_string(),
        ],
    };
    match run_one_protocol(capture_execute(|send| {
        PublishHighlightModule
            .execute(
                &nmp_core::substrate::ActionContext::default(),
                action,
                "chain-cid",
                send,
            )
            .expect("execute succeeds");
    })) {
        ActorCommand::Publish(PublishCommand::UnsignedEvent { event, .. }) => assert_eq!(
            event.tags,
            vec![
                vec![
                    "i".to_string(),
                    "bitcoin:tx:a1075db55d416d3ca199f55b6084e2115b9345e16c5cf302fc80e9d5fbf5d48d"
                        .to_string()
                ],
                vec![
                    "i".to_string(),
                    "ethereum:1:address:0xd8da6bf26964af9d7eed9e03e53415d37aa96045".to_string()
                ],
                vec!["k".to_string(), "bitcoin:tx".to_string()],
                vec!["k".to_string(), "ethereum:address".to_string()],
            ]
        ),
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}

#[test]
fn malformed_external_ids_are_rejected() {
    for external_id in [
        "",
        "https://",
        "podcast:item:guid:",
        "not-a-nip73-id",
        "doi:bad value",
        "doi:bad\nvalue",
        "geo:EZS42E44YX96",
        "iso3166:us-ca",
        "bitcoin:tx:",
        "ethereum::tx:abc",
        "bitcoin:block:abc",
    ] {
        let mut ctx = ActionContext::default();
        assert!(
            matches!(
                PublishHighlightModule.start(
                    &mut ctx,
                    PublishHighlightAction {
                        content: "clip text".to_string(),
                        context: None,
                        source_event_id: None,
                        source_address: None,
                        source_author_pubkey: None,
                        alt: None,
                        external_ids: vec![external_id.to_string()],
                    },
                ),
                Err(ActionRejection::Invalid(_))
            ),
            "{external_id:?} should be rejected"
        );
    }
}

#[test]
fn duplicate_derived_k_tags_are_deduped_from_ids() {
    let action = PublishHighlightAction {
        content: "references".to_string(),
        context: None,
        source_event_id: None,
        source_address: None,
        source_author_pubkey: None,
        alt: None,
        external_ids: vec![
            "https://example.com/one".to_string(),
            "http://example.com/two".to_string(),
            "isbn:9780765382030".to_string(),
        ],
    };
    match run_one_protocol(capture_execute(|send| {
        PublishHighlightModule
            .execute(
                &nmp_core::substrate::ActionContext::default(),
                action,
                "dedupe-cid",
                send,
            )
            .expect("execute succeeds");
    })) {
        ActorCommand::Publish(PublishCommand::UnsignedEvent { event, .. }) => assert_eq!(
            event.tags,
            vec![
                vec!["i".to_string(), "https://example.com/one".to_string()],
                vec!["i".to_string(), "http://example.com/two".to_string()],
                vec!["i".to_string(), "isbn:9780765382030".to_string()],
                vec!["k".to_string(), "web".to_string()],
                vec!["k".to_string(), "isbn".to_string()],
            ]
        ),
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}

#[test]
fn direct_protocol_run_rejects_malformed_external_id_defensively() {
    let cmd = capture_execute(|send| {
        PublishHighlightModule
            .execute(
                &nmp_core::substrate::ActionContext::default(),
                PublishHighlightAction {
                    content: "clip text".to_string(),
                    context: None,
                    source_event_id: None,
                    source_address: None,
                    source_author_pubkey: None,
                    alt: None,
                    external_ids: vec!["not-a-nip73-id".to_string()],
                },
                "bad-cid",
                send,
            )
            .expect("execute enqueues protocol command");
    });
    let ActorCommand::Protocol(cmd) = cmd else {
        panic!("expected protocol command");
    };
    let send = |_cmd| panic!("malformed command must not publish");
    let mut ctx = ProtocolCommandContext::with_send_only(&send);
    assert!(matches!(
        cmd.run(&mut ctx),
        Err(err) if err.message().contains("malformed highlight fields")
    ));
}
