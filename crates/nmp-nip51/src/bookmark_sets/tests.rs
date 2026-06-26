use super::*;
use nmp_core::substrate::EventId;

const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const EVENT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EVENT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ARTICLE: &str =
    "30023:cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee:article";

fn projection_for(active: Option<&str>) -> BookmarkSetsProjection {
    BookmarkSetsProjection::new(Arc::new(Mutex::new(active.map(str::to_string))))
}

fn event(author: &str, kind: u32, created_at: u64, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: EventId::from(format!("{created_at:0>64x}")),
        author: author.to_string(),
        kind,
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
fn projection_parses_bookmark_and_curation_sets() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&event(
        ALICE,
        KIND_BOOKMARK_SET,
        100,
        vec![
            vec!["d", "reading"],
            vec!["title", "Reading"],
            vec!["image", "https://example.com/cover.png"],
            vec!["description", "Useful references"],
            vec!["e", EVENT_A, "wss://relay.example/"],
            vec!["a", ARTICLE],
            vec!["t", "nostr"],
            vec!["e", "not-hex"],
        ],
    ));
    proj.on_kernel_event(&event(
        BOB,
        KIND_ARTICLE_CURATION_SET,
        101,
        vec![vec!["d", "essays"], vec!["a", ARTICLE], vec!["e", EVENT_B]],
    ));

    let snapshot = proj.snapshot();
    assert_eq!(snapshot.sets.len(), 2);

    let reading = proj
        .snapshot_for_set(ALICE, BookmarkSetKind::BookmarkSet, "reading")
        .expect("reading set");
    assert_eq!(reading.metadata.title.as_deref(), Some("Reading"));
    assert_eq!(
        reading.metadata.image.as_deref(),
        Some("https://example.com/cover.png")
    );
    assert_eq!(
        reading.items,
        vec![
            BookmarkItem::Event {
                event_id: EVENT_A.to_string(),
                relay: Some("wss://relay.example".to_string()),
            },
            BookmarkItem::Address {
                coordinate: ARTICLE.to_string(),
                relay: None,
            },
            BookmarkItem::Hashtag {
                hashtag: "nostr".to_string(),
            },
        ]
    );

    let curation = proj
        .snapshot_for_set(BOB, BookmarkSetKind::CurationSet, "essays")
        .expect("curation set");
    assert_eq!(
        curation.items,
        vec![
            BookmarkItem::Address {
                coordinate: ARTICLE.to_string(),
                relay: None,
            },
            item_event(EVENT_B)
        ]
    );
}

#[test]
fn projection_keys_by_author_kind_and_identifier_with_newest_wins() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&event(
        ALICE,
        KIND_BOOKMARK_SET,
        200,
        vec![vec!["d", "shared"], vec!["e", EVENT_A]],
    ));
    proj.on_kernel_event(&event(
        ALICE,
        KIND_BOOKMARK_SET,
        100,
        vec![vec!["d", "shared"], vec!["e", EVENT_B]],
    ));
    proj.on_kernel_event(&event(
        ALICE,
        KIND_ARTICLE_CURATION_SET,
        201,
        vec![vec!["d", "shared"], vec!["e", EVENT_B]],
    ));

    assert_eq!(
        proj.snapshot_for_set(ALICE, BookmarkSetKind::BookmarkSet, "shared")
            .expect("bookmark set")
            .items,
        vec![item_event(EVENT_A)]
    );
    assert_eq!(
        proj.snapshot_for_set(ALICE, BookmarkSetKind::CurationSet, "shared")
            .expect("curation set")
            .items,
        vec![item_event(EVENT_B)]
    );
}

#[test]
fn builder_emits_addressable_set_event() {
    let snapshot = BookmarkSetSnapshot {
        author: ALICE.to_string(),
        set_kind: BookmarkSetKind::CurationSet,
        identifier: "essays".to_string(),
        event_id: String::new(),
        created_at: 0,
        metadata: BookmarkListMetadata {
            title: Some("Essays".to_string()),
            image: None,
            description: Some("Long reads".to_string()),
        },
        items: vec![BookmarkItem::Address {
            coordinate: ARTICLE.to_string(),
            relay: Some("wss://relay.example".to_string()),
        }],
    };

    let unsigned = build_bookmark_set_event(&snapshot);
    assert_eq!(unsigned.kind, KIND_ARTICLE_CURATION_SET);
    assert_eq!(unsigned.created_at, 0);
    assert_eq!(
        unsigned.tags,
        vec![
            vec!["d", "essays"],
            vec!["title", "Essays"],
            vec!["description", "Long reads"],
            vec!["a", ARTICLE, "wss://relay.example"],
        ]
    );
}

#[test]
fn add_action_rejects_duplicate_and_publishes_append() {
    let projection = Arc::new(projection_for(Some(ALICE)));
    projection.on_kernel_event(&event(
        ALICE,
        KIND_BOOKMARK_SET,
        100,
        vec![vec!["d", "reading"], vec!["e", EVENT_A]],
    ));
    let action = AddBookmarkSetItemAction::new(Arc::clone(&projection));

    let duplicate = BookmarkSetUpdateInput {
        account_pubkey: ALICE.to_string(),
        set_kind: BookmarkSetKind::BookmarkSet,
        identifier: "reading".to_string(),
        item: item_event(EVENT_A),
    };
    assert!(matches!(
        action.start(&mut ActionContext::default(), duplicate),
        Err(ActionRejection::Conflict(_))
    ));

    let sent = Mutex::new(Vec::new());
    let input = BookmarkSetUpdateInput {
        account_pubkey: ALICE.to_string(),
        set_kind: BookmarkSetKind::BookmarkSet,
        identifier: "reading".to_string(),
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
    assert_eq!(event.kind, KIND_BOOKMARK_SET);
    assert_eq!(
        event.tags,
        vec![vec!["d", "reading"], vec!["e", EVENT_A], vec!["e", EVENT_B]]
    );
}

#[test]
fn remove_action_rejects_absent_and_publishes_removal() {
    let projection = Arc::new(projection_for(Some(ALICE)));
    projection.on_kernel_event(&event(
        ALICE,
        KIND_ARTICLE_CURATION_SET,
        100,
        vec![vec!["d", "essays"], vec!["e", EVENT_A], vec!["e", EVENT_B]],
    ));
    let action = RemoveBookmarkSetItemAction::new(Arc::clone(&projection));

    let absent = BookmarkSetUpdateInput {
        account_pubkey: ALICE.to_string(),
        set_kind: BookmarkSetKind::CurationSet,
        identifier: "essays".to_string(),
        item: BookmarkItem::Url {
            url: "https://absent.example".to_string(),
        },
    };
    assert!(matches!(
        action.start(&mut ActionContext::default(), absent),
        Err(ActionRejection::Conflict(_))
    ));

    let sent = Mutex::new(Vec::new());
    let input = BookmarkSetUpdateInput {
        account_pubkey: ALICE.to_string(),
        set_kind: BookmarkSetKind::CurationSet,
        identifier: "essays".to_string(),
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
    assert_eq!(event.kind, KIND_ARTICLE_CURATION_SET);
    assert_eq!(event.tags, vec![vec!["d", "essays"], vec!["e", EVENT_B]]);
}

#[test]
fn add_action_can_create_absent_set_and_rejects_bad_inputs() {
    let projection = Arc::new(projection_for(Some(ALICE)));
    let action = AddBookmarkSetItemAction::new(Arc::clone(&projection));

    let mismatch = BookmarkSetUpdateInput {
        account_pubkey: BOB.to_string(),
        set_kind: BookmarkSetKind::BookmarkSet,
        identifier: "reading".to_string(),
        item: item_event(EVENT_A),
    };
    assert!(matches!(
        action.start(&mut ActionContext::default(), mismatch),
        Err(ActionRejection::Unauthorized(_))
    ));

    let malformed = BookmarkSetUpdateInput {
        account_pubkey: ALICE.to_string(),
        set_kind: BookmarkSetKind::BookmarkSet,
        identifier: " ".to_string(),
        item: item_event(EVENT_A),
    };
    assert!(matches!(
        action.start(&mut ActionContext::default(), malformed),
        Err(ActionRejection::Invalid(_))
    ));

    let sent = Mutex::new(Vec::new());
    let input = BookmarkSetUpdateInput {
        account_pubkey: ALICE.to_string(),
        set_kind: BookmarkSetKind::BookmarkSet,
        identifier: "new".to_string(),
        item: item_event(EVENT_A),
    };
    action
        .execute(
            &nmp_core::substrate::ActionContext::default(),
            input,
            "corr-new",
            &|command| {
                sent.lock().expect("sent").push(command);
            },
        )
        .expect("execute create");
    let ActorCommand::Publish(PublishCommand::UnsignedEvent { event, .. }) =
        sent.lock().expect("sent").pop().expect("command")
    else {
        panic!("expected PublishUnsignedEvent");
    };
    assert_eq!(event.tags, vec![vec!["d", "new"], vec!["e", EVENT_A]]);
}
