use super::*;
use std::cell::RefCell;

const EVENT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
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

fn capture_execute(action: PublishHighlightInput) -> ActorCommand {
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    PublishHighlightModule
        .execute(action, "highlight-cid", &|cmd| {
            captured.borrow_mut().push(cmd)
        })
        .expect("execute succeeds");
    let mut cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1, "expected one command");
    cmds.pop().unwrap()
}

#[test]
fn publish_highlight_builds_kind_9802_event() {
    let action = PublishHighlightInput {
        highlighted_text: "a passage worth saving".to_string(),
        context: Some("full paragraph around the passage".to_string()),
        comment: Some("this is the useful part".to_string()),
        source_refs: vec![
            HighlightSource::Event {
                event_id: EVENT.to_string(),
                relay: Some("WSS://Relay.Example/".to_string()),
            },
            HighlightSource::Url {
                url: "https://example.com/article?utm_source=x".to_string(),
            },
        ],
        attributions: vec![HighlightAttribution {
            pubkey: AUTHOR.to_string(),
            relay: None,
            role: Some("author".to_string()),
        }],
    };

    match run_one_protocol(capture_execute(action)) {
        ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id,
            signer_pubkey,
        }) => {
            assert_eq!(event.kind, KIND_HIGHLIGHT);
            assert_eq!(event.created_at, 0);
            assert_eq!(event.pubkey, "");
            assert_eq!(event.content, "a passage worth saving");
            assert_eq!(
                event.tags,
                vec![
                    vec![
                        "e".to_string(),
                        EVENT.to_string(),
                        "wss://relay.example".to_string()
                    ],
                    vec![
                        "r".to_string(),
                        "https://example.com/article?utm_source=x".to_string(),
                        "source".to_string()
                    ],
                    vec![
                        "context".to_string(),
                        "full paragraph around the passage".to_string()
                    ],
                    vec!["comment".to_string(), "this is the useful part".to_string()],
                    vec![
                        "p".to_string(),
                        AUTHOR.to_string(),
                        String::new(),
                        "author".to_string()
                    ]
                ]
            );
            assert_eq!(correlation_id.as_deref(), Some("highlight-cid"));
            assert_eq!(signer_pubkey, None);
        }
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}

#[test]
fn publish_highlight_supports_nip73_podcast_external_refs() {
    let action = PublishHighlightInput {
        highlighted_text: String::new(),
        context: None,
        comment: None,
        source_refs: vec![HighlightSource::External {
            external_id: "podcast:item:guid:d98d189b-dc7b-45b1-8720-d4b98690f31f".to_string(),
            external_kind: "podcast:item:guid".to_string(),
            hint_url: Some("https://fountain.fm/episode/z1y9TMQRuqXl2awyrQxg".to_string()),
        }],
        attributions: Vec::new(),
    };

    let event = build_highlight_event(&action).expect("valid highlight");
    assert_eq!(
        event.tags,
        vec![
            vec![
                "i".to_string(),
                "podcast:item:guid:d98d189b-dc7b-45b1-8720-d4b98690f31f".to_string(),
                "https://fountain.fm/episode/z1y9TMQRuqXl2awyrQxg".to_string()
            ],
            vec!["k".to_string(), "podcast:item:guid".to_string()],
        ]
    );
}

#[test]
fn publish_highlight_rejects_malformed_source_ids() {
    let mut ctx = ActionContext::default();
    let err = PublishHighlightModule
        .start(
            &mut ctx,
            PublishHighlightInput {
                source_refs: vec![HighlightSource::Event {
                    event_id: "bad".to_string(),
                    relay: None,
                }],
                ..Default::default()
            },
        )
        .expect_err("bad event id rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn publish_highlight_rejects_empty_payload() {
    let mut ctx = ActionContext::default();
    let err = PublishHighlightModule
        .start(&mut ctx, PublishHighlightInput::default())
        .expect_err("empty highlight rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}
