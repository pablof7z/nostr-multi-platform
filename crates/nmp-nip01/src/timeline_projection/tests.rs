use super::*;
use nmp_content::{WireNode, WireNostrUriKind};
use nmp_nostr_id::{encode_note, encode_npub};
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
        relay_provenance: Vec::new(),
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
    assert!(matches!(snap.blocks[0], TimelineBlock::Standalone { .. }));
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
            TimelineBlock::Standalone {
                id: "new".to_string(),
                root: None
            },
            TimelineBlock::Standalone {
                id: "old".to_string(),
                root: None
            }
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
            TimelineBlock::Standalone {
                id: "new".to_string(),
                root: None
            },
            TimelineBlock::Standalone {
                id: "mid".to_string(),
                root: None
            }
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
    assert!(first.metrics.is_some(), "window snapshots carry metrics");
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
        vec![TimelineBlock::Standalone {
            id: "old".to_string(),
            root: None
        }]
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
        vec![TimelineBlock::Standalone {
            id: "root".to_string(),
            root: None
        }]
    );
    assert_eq!(
        snap.cards
            .iter()
            .map(|card| card.id.as_str())
            .collect::<Vec<_>>(),
        vec!["root", quoted_id.as_str()]
    );
}

/// ADR-0058 §8 6B removed the legacy `created_at` window-grow `load_older`
/// path from `ModularTimelineProjection` (the test-only, OP-feed-superseded
/// modular timeline): completeness now rides the seq-ordered pull pager
/// (`nmp_feed::PullFeedController`), and there is exactly one paging path. The
/// projection still windows its snapshot to the default limit; the deleted
/// `current_window_state_loads_older_inside_projection` test asserted the
/// removed grow behaviour and no longer applies.
#[test]
fn current_window_state_bounds_snapshot_to_default_limit() {
    let proj = ModularTimelineProjection::new(&spec());
    let total = DEFAULT_TIMELINE_WINDOW_LIMIT + 2;
    for idx in 0..total {
        let id = format!("id-{idx:03}");
        proj.on_kernel_event(&note(&id, (idx + 1) as u64, vec![]));
    }

    let first = proj.snapshot_current_window();
    assert_eq!(first.blocks.len(), DEFAULT_TIMELINE_WINDOW_LIMIT);
    assert!(
        first.page.as_ref().expect("page").has_more,
        "snapshot is bounded to the default window; older rows ride the pull pager"
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
        relay_provenance: Vec::new(),
    };
    let proj = ModularTimelineProjection::new(&spec_with_kinds(vec![1, nmp_nip18::KIND_REPOST]));

    proj.on_kernel_event(&repost);
    let snap = proj.snapshot();
    let card = snap
        .cards
        .iter()
        .find(|c| c.id == "repost")
        .expect("repost card exists");

    // Card surfaces the *original* note (kind:1) — the kind:6 wrapper is
    // exposed via `reposted_by`. Sort key (`created_at`) stays as the
    // repost's timestamp so the card bumps to the top of the feed.
    assert_eq!(card.kind, 1);
    assert_eq!(card.author_pubkey, "inner-author");
    assert_eq!(card.created_at, 2, "sort key is the outer repost timestamp");
    assert_eq!(card.content, "boosted #nostr");
    assert!(card
        .content_tree
        .nodes
        .iter()
        .any(|node| { matches!(node, WireNode::Hashtag { tag } if tag == "nostr") }));

    let attribution = card
        .reposted_by
        .as_ref()
        .expect("repost attribution present");
    assert_eq!(attribution.author_pubkey, "reposter");
    assert_eq!(
        attribution.note_created_at, 123,
        "attribution carries the original note's publish time"
    );
}

#[test]
fn ordinary_notes_have_no_repost_attribution() {
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note("N", 1, vec![]));
    let snap = proj.snapshot();
    let card = snap
        .cards
        .iter()
        .find(|c| c.id == "N")
        .expect("note card exists");
    assert!(card.reposted_by.is_none());
}

#[test]
fn observer_trait_object_drives_grouper() {
    let proj: Arc<dyn ObservedProjectionSink> = Arc::new(ModularTimelineProjection::new(&spec()));
    proj.on_kernel_event(&note("X", 1, vec![]));
}

// ── Raw-data card tests (aim.md §2 — no denormalized display) ────────

#[test]
fn card_carries_raw_pubkey_and_no_denormalized_display() {
    // GH #920: the card no longer denormalizes any kind:0 display copy. The
    // raw hex pubkey is the only author identity it carries; the presentation
    // layer joins against the snapshot's `refs.profile` map. A kind:0 for
    // the author is inert for this projection (it produces no card and does
    // not mutate the existing one).
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&KernelEvent {
        id: "E".into(),
        author: PK.into(),
        kind: 1,
        created_at: 1,
        tags: vec![],
        content: "hello".into(),
        relay_provenance: Vec::new(),
    });
    let pre = proj
        .snapshot()
        .cards
        .into_iter()
        .find(|c| c.id == "E")
        .expect("card exists");
    // Raw hex pubkey passes through verbatim.
    assert_eq!(pre.author_pubkey, PK);
    assert_eq!(pre.content, "hello");

    // A later kind:0 does not add a card and leaves the existing one unchanged.
    proj.on_kernel_event(&KernelEvent {
        id: "P".into(),
        author: PK.into(),
        kind: 0,
        created_at: 2,
        tags: vec![],
        content: r#"{"display_name":"Alice","picture":"https://example.com/a.png"}"#.into(),
        relay_provenance: Vec::new(),
    });
    let post = proj.snapshot();
    assert_eq!(post.cards.len(), 1, "kind:0 produces no card");
    let card = post.cards.into_iter().find(|c| c.id == "E").expect("card");
    assert_eq!(card.author_pubkey, PK);
}
