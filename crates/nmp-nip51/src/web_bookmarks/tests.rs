use super::*;
use nmp_core::substrate::EventId;

const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

fn projection_for(active: Option<&str>) -> WebBookmarksProjection {
    WebBookmarksProjection::new(Arc::new(Mutex::new(active.map(str::to_string))))
}

fn event(author: &str, created_at: u64, tags: Vec<Vec<&str>>, content: &str) -> KernelEvent {
    KernelEvent {
        id: EventId::from(format!("{created_at:0>64x}")),
        author: author.to_string(),
        kind: KIND_WEB_BOOKMARK,
        created_at,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn projection_parses_web_bookmark_facts() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&event(
        ALICE,
        100,
        vec![
            vec!["d", "alice.blog/post"],
            vec!["published_at", "99"],
            vec!["title", "Blog insights"],
            vec!["t", "#nostr"],
            vec!["t", "nostr"],
            vec!["t", "writing"],
        ],
        "A useful article.",
    ));

    let snapshot = proj.snapshot();
    assert_eq!(snapshot.bookmarks.len(), 1);
    let bookmark = proj
        .snapshot_for_bookmark(ALICE, "alice.blog/post")
        .expect("web bookmark");
    assert_eq!(bookmark.title.as_deref(), Some("Blog insights"));
    assert_eq!(bookmark.published_at, Some(99));
    assert_eq!(bookmark.description, "A useful article.");
    assert_eq!(bookmark.hashtags, vec!["nostr", "writing"]);
}

#[test]
fn projection_ignores_malformed_d_and_newest_event_wins() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&event(
        ALICE,
        100,
        vec![vec!["d", "https://alice.blog/post"]],
        "ignored",
    ));
    assert!(proj.snapshot().bookmarks.is_empty());

    proj.on_kernel_event(&event(
        ALICE,
        200,
        vec![vec!["d", "alice.blog/post"], vec!["title", "new"]],
        "new",
    ));
    proj.on_kernel_event(&event(
        ALICE,
        150,
        vec![vec!["d", "alice.blog/post"], vec!["title", "old"]],
        "old",
    ));
    assert_eq!(
        proj.snapshot_for_bookmark(ALICE, "alice.blog/post")
            .expect("web bookmark")
            .title
            .as_deref(),
        Some("new")
    );
}

#[test]
fn snapshot_for_authors_filters_explicit_author_set() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&event(ALICE, 100, vec![vec!["d", "alice.blog/post"]], ""));
    proj.on_kernel_event(&event(BOB, 101, vec![vec!["d", "bob.blog/post"]], ""));

    let snapshot = proj.snapshot_for_authors([BOB]);
    assert_eq!(snapshot.bookmarks.len(), 1);
    assert_eq!(snapshot.bookmarks[0].author, BOB);
    assert_eq!(snapshot.bookmarks[0].url_without_scheme, "bob.blog/post");
}

#[test]
fn builder_strips_scheme_and_emits_metadata() {
    let draft = WebBookmarkDraft {
        url: "https://alice.blog/post".to_string(),
        title: Some("Blog insights".to_string()),
        description: Some("A useful article.".to_string()),
        published_at: Some(99),
        hashtags: vec![
            "#nostr".to_string(),
            "nostr".to_string(),
            "writing".to_string(),
        ],
    };

    let unsigned = build_web_bookmark_event(&draft).expect("valid bookmark");
    assert_eq!(unsigned.kind, KIND_WEB_BOOKMARK);
    assert_eq!(unsigned.created_at, 0);
    assert_eq!(unsigned.content, "A useful article.");
    assert_eq!(
        unsigned.tags,
        vec![
            vec!["d", "alice.blog/post"],
            vec!["published_at", "99"],
            vec!["title", "Blog insights"],
            vec!["t", "nostr"],
            vec!["t", "writing"],
        ]
    );
}

#[test]
fn builder_rejects_non_http_url() {
    let draft = WebBookmarkDraft {
        url: "ftp://example.com/file".to_string(),
        title: None,
        description: None,
        published_at: None,
        hashtags: Vec::new(),
    };
    assert!(build_web_bookmark_event(&draft).is_err());
}

#[test]
fn publish_action_rejects_bad_inputs_and_publishes_upsert() {
    let projection = Arc::new(projection_for(Some(ALICE)));
    let action = PublishWebBookmarkAction::new(Arc::clone(&projection));

    let mismatch = PublishWebBookmarkInput {
        account_pubkey: BOB.to_string(),
        bookmark: WebBookmarkDraft {
            url: "https://alice.blog/post".to_string(),
            title: None,
            description: None,
            published_at: None,
            hashtags: Vec::new(),
        },
    };
    assert!(matches!(
        action.start(&mut ActionContext::default(), mismatch),
        Err(ActionRejection::Unauthorized(_))
    ));

    let malformed = PublishWebBookmarkInput {
        account_pubkey: ALICE.to_string(),
        bookmark: WebBookmarkDraft {
            url: "alice.blog/post".to_string(),
            title: None,
            description: None,
            published_at: None,
            hashtags: Vec::new(),
        },
    };
    assert!(matches!(
        action.start(&mut ActionContext::default(), malformed),
        Err(ActionRejection::Invalid(_))
    ));

    let sent = Mutex::new(Vec::new());
    let input = PublishWebBookmarkInput {
        account_pubkey: ALICE.to_string(),
        bookmark: WebBookmarkDraft {
            url: "http://alice.blog/post".to_string(),
            title: Some("Blog".to_string()),
            description: None,
            published_at: None,
            hashtags: Vec::new(),
        },
    };
    action
        .start(&mut ActionContext::default(), input.clone())
        .expect("valid publish");
    action
        .execute(
            &nmp_core::substrate::ActionContext::default(),
            input,
            "corr-web",
            &|command| {
                sent.lock().expect("sent").push(command);
            },
        )
        .expect("execute publish");

    let ActorCommand::Publish(PublishCommand::UnsignedEvent {
        event,
        correlation_id,
        signer_pubkey,
    }) = sent.lock().expect("sent").pop().expect("command")
    else {
        panic!("expected PublishUnsignedEvent");
    };
    assert_eq!(correlation_id.as_deref(), Some("corr-web"));
    assert_eq!(signer_pubkey, None);
    assert_eq!(event.kind, KIND_WEB_BOOKMARK);
    assert_eq!(
        event.tags,
        vec![vec!["d", "alice.blog/post"], vec!["title", "Blog"]]
    );
}
