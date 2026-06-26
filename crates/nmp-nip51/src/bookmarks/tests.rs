use super::*;
use nmp_core::substrate::EventId;

const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const EVENT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EVENT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ARTICLE: &str =
    "30023:cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee:article";

fn projection_for(active: Option<&str>) -> BookmarkListProjection {
    BookmarkListProjection::new(Arc::new(Mutex::new(active.map(str::to_string))))
}

fn event(author: &str, created_at: u64, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: EventId::from(
            "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        ),
        author: author.to_string(),
        kind: KIND_BOOKMARK_LIST,
        created_at,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn item_event(id: &str) -> BookmarkItem {
    BookmarkItem::Event {
        event_id: id.to_string(),
        relay: None,
    }
}

#[test]
fn projection_parses_raw_bookmark_facts() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&event(
        ALICE,
        100,
        vec![
            vec!["title", "Saved"],
            vec!["image", "https://example.com/image.png"],
            vec!["description", "Global list"],
            vec!["e", EVENT_A, "wss://relay.example/"],
            vec!["a", ARTICLE],
            vec!["r", "https://example.com/page"],
            vec!["t", "nostr"],
            vec!["e", "not-hex"],
            vec!["r", "ftp://ignored.example"],
        ],
    ));

    let snapshot = proj.snapshot();
    assert_eq!(snapshot.metadata.title.as_deref(), Some("Saved"));
    assert_eq!(
        snapshot.metadata.image.as_deref(),
        Some("https://example.com/image.png")
    );
    assert_eq!(
        snapshot.metadata.description.as_deref(),
        Some("Global list")
    );
    assert_eq!(
        snapshot.items,
        vec![
            BookmarkItem::Event {
                event_id: EVENT_A.to_string(),
                relay: Some("wss://relay.example".to_string()),
            },
            BookmarkItem::Address {
                coordinate: ARTICLE.to_string(),
                relay: None,
            },
            BookmarkItem::Url {
                url: "https://example.com/page".to_string(),
            },
            BookmarkItem::Hashtag {
                hashtag: "nostr".to_string(),
            },
        ]
    );
}

#[test]
fn projection_is_active_account_gated_and_newer_event_wins() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&event(BOB, 100, vec![vec!["e", EVENT_A]]));
    assert_eq!(proj.snapshot(), BookmarkListSnapshot::default());

    proj.on_kernel_event(&event(ALICE, 101, vec![vec!["e", EVENT_A]]));
    proj.on_kernel_event(&event(ALICE, 100, vec![vec!["e", EVENT_B]]));
    assert_eq!(proj.snapshot().items, vec![item_event(EVENT_A)]);

    proj.on_kernel_event(&event(ALICE, 102, vec![vec!["e", EVENT_B]]));
    assert_eq!(proj.snapshot().items, vec![item_event(EVENT_B)]);
}

#[test]
fn account_switch_hides_stale_bookmarks() {
    let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
    let proj = BookmarkListProjection::new(Arc::clone(&slot));
    proj.on_kernel_event(&event(ALICE, 100, vec![vec!["e", EVENT_A]]));
    assert!(!proj.snapshot().items.is_empty());

    *slot.lock().expect("slot") = Some(BOB.to_string());
    assert_eq!(proj.snapshot(), BookmarkListSnapshot::default());
}

#[test]
fn builder_emits_metadata_and_items_with_created_at_sentinel() {
    let snapshot = BookmarkListSnapshot {
        metadata: BookmarkListMetadata {
            title: Some("Saved".to_string()),
            image: None,
            description: Some("Global list".to_string()),
        },
        items: vec![
            BookmarkItem::Event {
                event_id: EVENT_A.to_string(),
                relay: Some("wss://relay.example".to_string()),
            },
            BookmarkItem::Address {
                coordinate: ARTICLE.to_string(),
                relay: None,
            },
            BookmarkItem::Url {
                url: "https://example.com/page".to_string(),
            },
            BookmarkItem::Hashtag {
                hashtag: "nostr".to_string(),
            },
        ],
    };

    let unsigned = build_bookmark_list_event(&snapshot);
    assert_eq!(unsigned.kind, KIND_BOOKMARK_LIST);
    assert_eq!(unsigned.created_at, 0);
    assert_eq!(
        unsigned.tags,
        vec![
            vec!["title", "Saved"],
            vec!["description", "Global list"],
            vec!["e", EVENT_A, "wss://relay.example"],
            vec!["a", ARTICLE],
            vec!["r", "https://example.com/page"],
            vec!["t", "nostr"],
        ]
    );
}

#[test]
fn add_action_rejects_duplicate_and_publishes_append() {
    let projection = Arc::new(projection_for(Some(ALICE)));
    projection.on_kernel_event(&event(ALICE, 100, vec![vec!["e", EVENT_A]]));
    let action = AddBookmarkAction::new(Arc::clone(&projection));

    let duplicate = BookmarkUpdateInput {
        account_pubkey: ALICE.to_string(),
        item: item_event(EVENT_A),
    };
    assert!(matches!(
        action.start(&mut ActionContext::default(), duplicate),
        Err(ActionRejection::Conflict(_))
    ));

    let sent = Mutex::new(Vec::new());
    let input = BookmarkUpdateInput {
        account_pubkey: ALICE.to_string(),
        item: item_event(EVENT_B),
    };
    action
        .start(&mut ActionContext::default(), input.clone())
        .expect("valid add");
    action
        .execute(
            &nmp_core::substrate::ActionContext::default(),
            input,
            "corr-add",
            &|command| {
                sent.lock().expect("sent").push(command);
            },
        )
        .expect("execute add");

    let ActorCommand::Publish(PublishCommand::UnsignedEvent {
        event,
        correlation_id,
        signer_pubkey,
    }) = sent.lock().expect("sent").pop().expect("command")
    else {
        panic!("expected PublishUnsignedEvent");
    };
    assert_eq!(correlation_id.as_deref(), Some("corr-add"));
    assert_eq!(signer_pubkey, None);
    assert_eq!(event.kind, KIND_BOOKMARK_LIST);
    assert_eq!(event.tags, vec![vec!["e", EVENT_A], vec!["e", EVENT_B]]);
}

#[test]
fn remove_action_rejects_absent_and_publishes_removal() {
    let projection = Arc::new(projection_for(Some(ALICE)));
    projection.on_kernel_event(&event(
        ALICE,
        100,
        vec![vec!["e", EVENT_A], vec!["e", EVENT_B]],
    ));
    let action = RemoveBookmarkAction::new(Arc::clone(&projection));

    let absent = BookmarkUpdateInput {
        account_pubkey: ALICE.to_string(),
        item: BookmarkItem::Url {
            url: "https://absent.example".to_string(),
        },
    };
    assert!(matches!(
        action.start(&mut ActionContext::default(), absent),
        Err(ActionRejection::Conflict(_))
    ));

    let sent = Mutex::new(Vec::new());
    let input = BookmarkUpdateInput {
        account_pubkey: ALICE.to_string(),
        item: item_event(EVENT_A),
    };
    action
        .start(&mut ActionContext::default(), input.clone())
        .expect("valid remove");
    action
        .execute(
            &nmp_core::substrate::ActionContext::default(),
            input,
            "corr-remove",
            &|command| {
                sent.lock().expect("sent").push(command);
            },
        )
        .expect("execute remove");

    let ActorCommand::Publish(PublishCommand::UnsignedEvent { event, .. }) =
        sent.lock().expect("sent").pop().expect("command")
    else {
        panic!("expected PublishUnsignedEvent");
    };
    assert_eq!(event.tags, vec![vec!["e", EVENT_B]]);
}

#[test]
fn action_rejects_mismatched_active_account_and_malformed_item() {
    let projection = Arc::new(projection_for(Some(ALICE)));
    let action = AddBookmarkAction::new(projection);

    let mismatch = BookmarkUpdateInput {
        account_pubkey: BOB.to_string(),
        item: item_event(EVENT_A),
    };
    assert!(matches!(
        action.start(&mut ActionContext::default(), mismatch),
        Err(ActionRejection::Unauthorized(_))
    ));

    let malformed = BookmarkUpdateInput {
        account_pubkey: ALICE.to_string(),
        item: BookmarkItem::Event {
            event_id: "not-hex".to_string(),
            relay: None,
        },
    };
    assert!(matches!(
        action.start(&mut ActionContext::default(), malformed),
        Err(ActionRejection::Invalid(_))
    ));
}
