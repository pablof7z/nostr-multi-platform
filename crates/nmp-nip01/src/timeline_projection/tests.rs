use super::*;
use nmp_content::{WireNode, WireNostrUriKind};
use nmp_core::nip19::{encode_note, encode_npub};
use nmp_threading::{ModulePolicy, TimelineBlock};
use std::sync::Arc;

fn spec() -> ModularTimelineSpec {
    spec_with_kinds(vec![])
}

fn spec_with_kinds(kinds: Vec<u32>) -> ModularTimelineSpec {
    ModularTimelineSpec {
        viewer: "me".into(),
        kinds,
        authors: None,
        policy: ModulePolicy::default(),
    }
}

fn note(id: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    note_with_content(id, ts, tags, id)
}

fn note_with_content(id: &str, ts: u64, tags: Vec<Vec<String>>, content: &str) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: "auth".into(),
        kind: 1,
        created_at: ts,
        tags,
        content: content.into(),
    }
}

fn reply_to(id: &str, ts: u64, root: &str, parent: &str) -> KernelEvent {
    note(
        id,
        ts,
        vec![
            vec!["e".into(), root.into(), "".into(), "root".into()],
            vec!["e".into(), parent.into(), "".into(), "reply".into()],
        ],
    )
}

#[test]
fn empty_open_yields_empty_snapshot() {
    let proj = ModularTimelineProjection::new(&spec());
    let snap = proj.snapshot();
    assert!(snap.blocks.is_empty());
    assert!(snap.cards.is_empty());
}

#[test]
fn root_plus_reply_collapses_into_one_module() {
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note("R", 1, vec![]));
    proj.on_kernel_event(&reply_to("C", 2, "R", "R"));
    let snap = proj.snapshot();
    assert_eq!(snap.blocks.len(), 1);
    match &snap.blocks[0] {
        TimelineBlock::Module { events, .. } => {
            assert_eq!(events, &vec!["R".to_string(), "C".to_string()]);
        }
        other => panic!("expected Module, got {other:?}"),
    }
    assert_eq!(snap.cards.len(), 2);
}

#[test]
fn standalone_event_becomes_standalone_block() {
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note("S", 1, vec![]));
    let snap = proj.snapshot();
    assert_eq!(snap.blocks.len(), 1);
    assert!(matches!(snap.blocks[0], TimelineBlock::Standalone(_)));
}

#[test]
fn snapshot_sorts_backfilled_events_newest_first() {
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note("new", 3, vec![]));
    proj.on_kernel_event(&note("old", 1, vec![]));

    let snap = proj.snapshot();

    assert_eq!(
        snap.blocks,
        vec![
            TimelineBlock::Standalone("new".to_string()),
            TimelineBlock::Standalone("old".to_string())
        ]
    );
}

#[test]
fn window_snapshot_pages_blocks_with_stable_cursor() {
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note("old", 1, vec![]));
    proj.on_kernel_event(&note("mid", 2, vec![]));
    proj.on_kernel_event(&note("new", 3, vec![]));

    let first = proj.snapshot_window(TimelineWindowRequest::newest(2));

    assert_eq!(
        first.blocks,
        vec![
            TimelineBlock::Standalone("new".to_string()),
            TimelineBlock::Standalone("mid".to_string())
        ]
    );
    assert_eq!(
        first
            .cards
            .iter()
            .map(|card| card.id.as_str())
            .collect::<Vec<_>>(),
        vec!["new", "mid"]
    );
    let page = first.page.expect("window snapshots carry page metadata");
    assert!(page.has_more);
    assert_eq!(page.total_blocks, 3);
    assert_eq!(
        page.next_cursor,
        Some(TimelineWindowCursor {
            created_at: 2,
            id: "mid".to_string()
        })
    );

    let second = proj.snapshot_window(TimelineWindowRequest {
        limit: 2,
        cursor: page.next_cursor,
    });

    assert_eq!(
        second.blocks,
        vec![TimelineBlock::Standalone("old".to_string())]
    );
    assert!(!second.page.expect("page").has_more);
}

#[test]
fn window_snapshot_includes_visible_quote_cards() {
    let quoted_id = "b".repeat(64);
    let note_uri = format!(
        "nostr:{}",
        encode_note(&quoted_id).expect("fixture note id encodes")
    );
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note_with_content(&quoted_id, 1, vec![], "quoted"));
    proj.on_kernel_event(&note_with_content(
        "root",
        2,
        vec![],
        &format!("quote {note_uri}"),
    ));

    let snap = proj.snapshot_window(TimelineWindowRequest::newest(1));

    assert_eq!(
        snap.blocks,
        vec![TimelineBlock::Standalone("root".to_string())]
    );
    assert_eq!(
        snap.cards
            .iter()
            .map(|card| card.id.as_str())
            .collect::<Vec<_>>(),
        vec!["root", quoted_id.as_str()]
    );
}

#[test]
fn cards_include_content_tree_wire_for_mentions() {
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let mention = format!("nostr:{}", encode_npub(PK).expect("fixture npub encodes"));
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note_with_content(
        "S",
        1,
        vec![],
        &format!("hello {mention} #nostr"),
    ));

    let snap = proj.snapshot();
    let card = snap
        .cards
        .iter()
        .find(|c| c.id == "S")
        .expect("card exists");
    assert!(card.content_tree.nodes.iter().any(|node| {
        matches!(
            node,
            WireNode::Mention { uri }
                if uri.kind == WireNostrUriKind::Profile && uri.primary_id == PK
        )
    }));
}

#[test]
fn content_render_profiles_refresh_when_kind0_arrives_later() {
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let mention = format!("nostr:{}", encode_npub(PK).expect("fixture npub encodes"));
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note_with_content(
        "S",
        1,
        vec![],
        &format!("hello {mention}"),
    ));

    let pre = proj
        .snapshot()
        .cards
        .into_iter()
        .find(|c| c.id == "S")
        .expect("card exists");
    assert_eq!(
        pre.content_render
            .profiles
            .get(PK)
            .and_then(|profile| profile.display.name.as_deref()),
        None
    );

    proj.on_kernel_event(&KernelEvent {
        id: "P".into(),
        author: PK.into(),
        kind: 0,
        created_at: 2,
        tags: vec![],
        content: r#"{"display_name":"Bob","picture":"https://example.com/b.png"}"#.into(),
    });
    let post = proj
        .snapshot()
        .cards
        .into_iter()
        .find(|c| c.id == "S")
        .expect("card exists");
    let profile = post
        .content_render
        .profiles
        .get(PK)
        .expect("mention profile");
    assert_eq!(profile.display.name.as_deref(), Some("Bob"));
    assert_eq!(
        profile.display.picture_url.as_deref(),
        Some("https://example.com/b.png")
    );
}

#[test]
fn content_render_events_refresh_when_quoted_event_arrives_later() {
    let quoted_id = "b".repeat(64);
    let note_uri = format!(
        "nostr:{}",
        encode_note(&quoted_id).expect("fixture note id encodes")
    );
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note_with_content(
        "root",
        1,
        vec![],
        &format!("quote {note_uri}"),
    ));
    let pre = proj
        .snapshot()
        .cards
        .into_iter()
        .find(|c| c.id == "root")
        .expect("card exists");
    assert!(
        pre.content_render.events.is_empty(),
        "quote cannot render until the referenced event is in the projection"
    );

    proj.on_kernel_event(&KernelEvent {
        id: quoted_id.clone(),
        author: "c".repeat(64),
        kind: 1,
        created_at: 2,
        tags: vec![],
        content: "quoted note body #nostr".into(),
    });
    let post = proj
        .snapshot()
        .cards
        .into_iter()
        .find(|c| c.id == "root")
        .expect("card exists");
    let quote = post
        .content_render
        .events
        .get(&quoted_id)
        .expect("resolved quote event");
    assert_eq!(quote.content_preview, "quoted note body #nostr");
    assert!(quote
        .content_tree
        .nodes
        .iter()
        .any(|node| matches!(node, WireNode::Hashtag { tag } if tag == "nostr")));
}

#[test]
fn repost_cards_render_embedded_event_content_tree() {
    let embedded = serde_json::json!({
        "id": "inner",
        "pubkey": "inner-author",
        "kind": 1,
        "created_at": 123,
        "tags": [],
        "content": "boosted #nostr",
        "sig": "ignored"
    });
    let repost = KernelEvent {
        id: "repost".into(),
        author: "reposter".into(),
        kind: nmp_nip18::KIND_REPOST,
        created_at: 2,
        tags: vec![vec!["e".into(), "inner".into()]],
        content: embedded.to_string(),
    };
    let proj = ModularTimelineProjection::new(&spec_with_kinds(vec![1, nmp_nip18::KIND_REPOST]));

    proj.on_kernel_event(&repost);
    let snap = proj.snapshot();
    let card = snap
        .cards
        .iter()
        .find(|c| c.id == "repost")
        .expect("repost card exists");

    assert_eq!(card.kind, nmp_nip18::KIND_REPOST);
    assert_eq!(card.content, "boosted #nostr");
    assert!(card
        .content_tree
        .nodes
        .iter()
        .any(|node| { matches!(node, WireNode::Hashtag { tag } if tag == "nostr") }));
}

#[test]
fn observer_trait_object_drives_grouper() {
    let proj: Arc<dyn KernelEventObserver> = Arc::new(ModularTimelineProjection::new(&spec()));
    proj.on_kernel_event(&note("X", 1, vec![]));
}

// ── Raw-data display-field tests (aim.md §2) ─────────────────────────

#[test]
fn card_with_no_profile_yields_optional_fields_as_none() {
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let event = KernelEvent {
        id: "E".into(),
        author: PK.into(),
        kind: 1,
        created_at: 1,
        tags: vec![],
        content: "hello".into(),
    };
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&event);
    let snap = proj.snapshot();
    let card = snap
        .cards
        .iter()
        .find(|c| c.id == "E")
        .expect("card exists");

    // Raw hex pubkey passes through verbatim.
    assert_eq!(card.author_pubkey, PK);
    // No kind:0 has arrived yet → display_name / picture_url are None.
    assert_eq!(card.author_display_name, None);
    assert_eq!(card.author_picture_url, None);
    assert_eq!(card.author_display.name, None);
    assert_eq!(card.author_display.picture_url, None);
    // npub is pubkey-deterministic, always present.
    assert!(card
        .author_display
        .npub
        .as_deref()
        .unwrap()
        .starts_with("npub1"));
}

#[test]
fn refresh_author_cards_populates_display_name_when_kind0_arrives_later() {
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let proj = ModularTimelineProjection::new(&spec());
    let note_event = KernelEvent {
        id: "E".into(),
        author: PK.into(),
        kind: 1,
        created_at: 1,
        tags: vec![],
        content: "hi".into(),
    };
    proj.on_kernel_event(&note_event);
    let pre = proj
        .snapshot()
        .cards
        .into_iter()
        .find(|c| c.id == "E")
        .expect("card");
    assert_eq!(pre.author_display_name, None);

    let profile_event = KernelEvent {
        id: "P".into(),
        author: PK.into(),
        kind: 0,
        created_at: 2,
        tags: vec![],
        content: r#"{"display_name":"Alice"}"#.into(),
    };
    proj.on_kernel_event(&profile_event);
    let post = proj
        .snapshot()
        .cards
        .into_iter()
        .find(|c| c.id == "E")
        .expect("card");
    assert_eq!(post.author_display_name.as_deref(), Some("Alice"));
    assert_eq!(post.author_display.name.as_deref(), Some("Alice"));
}

#[test]
fn refresh_author_cards_populates_picture_url_when_kind0_arrives_later() {
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&KernelEvent {
        id: "E".into(),
        author: PK.into(),
        kind: 1,
        created_at: 1,
        tags: vec![],
        content: "hi".into(),
    });
    let pre = proj
        .snapshot()
        .cards
        .into_iter()
        .find(|c| c.id == "E")
        .expect("card");
    assert_eq!(pre.author_picture_url, None);

    proj.on_kernel_event(&KernelEvent {
        id: "P".into(),
        author: PK.into(),
        kind: 0,
        created_at: 2,
        tags: vec![],
        content: r#"{"display_name":"Alice","picture":"https://example.com/a.png"}"#.into(),
    });
    let post = proj
        .snapshot()
        .cards
        .into_iter()
        .find(|c| c.id == "E")
        .expect("card");
    assert_eq!(
        post.author_picture_url.as_deref(),
        Some("https://example.com/a.png")
    );
    assert_eq!(post.author_display.picture_url, post.author_picture_url);
}

#[test]
fn content_preview_truncates_at_180_scalars_without_ellipsis() {
    // 200-char ASCII body → preview is the first 180 chars, no `…`.
    let body = "a".repeat(200);
    let expected = "a".repeat(180);
    let event = KernelEvent {
        id: "L".into(),
        author: "a".repeat(64),
        kind: 1,
        created_at: 1,
        tags: vec![],
        content: body,
    };
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&event);
    let card = proj
        .snapshot()
        .cards
        .into_iter()
        .find(|c| c.id == "L")
        .expect("card");
    assert_eq!(card.content_preview.len(), 180);
    assert_eq!(card.content_preview, expected);
    assert!(!card.content_preview.ends_with('…'));
}
