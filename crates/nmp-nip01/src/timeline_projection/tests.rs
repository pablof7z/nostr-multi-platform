use super::*;
use nmp_content::{WireNode, WireNostrUriKind};
use nmp_core::nip19::encode_npub;
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

// ── V-27 thin-shell display-field tests ──────────────────────────────

#[test]
fn card_carries_v27_display_fields_for_ingested_event() {
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

    // initials: ".." placeholder until Kind0 lands (V-34)
    assert_eq!(card.author_avatar_initials, "..");
    // colour: deterministic djb2 hex, 6 uppercase hex chars, no `#`
    assert_eq!(card.author_avatar_color.len(), 6);
    assert!(card
        .author_avatar_color
        .chars()
        .all(|c| c.is_ascii_hexdigit() && (c.is_ascii_digit() || c.is_ascii_uppercase())));
    // pubkey short: 8…8 with ellipsis when hex is long
    assert_eq!(card.author_pubkey_short, "3bf0c63f…aefa459d");
    // created_at_display: a pinned old timestamp resolves to "Xd ago"
    // (test runs well after 1970 so the bucket is `d`).
    assert!(
        card.created_at_display.ends_with(" ago"),
        "expected `Xd ago`, got {}",
        card.created_at_display
    );
    // flat display-name mirror equals nested AuthorDisplay.name.
    assert_eq!(card.author_display_name, card.author_display.name);
    assert!(!card.author_display_name.is_empty());
}

// The canonical pinned djb2 vector and exhaustive `format_ago_secs`
// bucket coverage live in `nmp_core::display::tests` (V-33). The
// `card_carries_v27_display_fields_for_ingested_event` test above pins
// the call-site result (`PK = "3bf0…"` → `card.author_avatar_color`)
// so a drift in the canonical helper still surfaces at this layer.

#[test]
fn display_name_initials_word_based() {
    // word-based: first char of each word, uppercase (canonical algorithm)
    assert_eq!(display_name_initials("Alice Smith"), "AS");
    assert_eq!(display_name_initials("alice bob"), "AB");
    assert_eq!(display_name_initials("bob"), "B.");
    assert_eq!(display_name_initials("a"), "A.");
    assert_eq!(display_name_initials(""), "..");
}

#[test]
fn short_hex_short_inputs_returned_unchanged() {
    assert_eq!(short_hex(""), "");
    assert_eq!(short_hex("abcd"), "abcd");
    // boundary: exactly 16 chars triggers abbreviation
    assert_eq!(short_hex("0123456789abcdef"), "01234567…89abcdef");
}

#[test]
fn refresh_author_cards_updates_v27_display_name_when_kind0_arrives_later() {
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let proj = ModularTimelineProjection::new(&spec());
    // First the note arrives with no profile loaded — display_name is the npub fallback.
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
    assert!(pre.author_display_name.starts_with("npub1"));

    // Then a kind:0 arrives — the flat mirror must update.
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
    assert_eq!(post.author_display_name, "Alice");
    assert_eq!(post.author_display.name, "Alice");
}

// ── V-32 thin-shell tests ───────────────────────────────────────────

#[test]
fn card_carries_v32_picture_url_and_content_preview() {
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let event = KernelEvent {
        id: "E".into(),
        author: PK.into(),
        kind: 1,
        created_at: 1,
        tags: vec![],
        content: "hello world".into(),
    };
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&event);
    let snap = proj.snapshot();
    let card = snap
        .cards
        .iter()
        .find(|c| c.id == "E")
        .expect("card exists");

    // No profile loaded yet → identicon placeholder from nmp-core
    // (`picture_placeholder` uses the first 16 hex chars, NOT 8 —
    // deliberate alignment with the cross-surface placeholder).
    assert_eq!(card.author_picture_url, "identicon:3bf0c63fcb934634");
    // Field must equal the nested `AuthorDisplay.picture_url` —
    // single source of truth.
    assert_eq!(card.author_picture_url, card.author_display.picture_url);

    // content_preview: short content passes through unchanged, no ellipsis.
    assert_eq!(card.content_preview, "hello world");
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

#[test]
fn refresh_author_cards_updates_v32_picture_url_when_kind0_arrives_later() {
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let proj = ModularTimelineProjection::new(&spec());
    // Note arrives first → identicon placeholder.
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
    assert!(pre.author_picture_url.starts_with("identicon:"));

    // Kind:0 with a real picture URL arrives — the flat mirror must update.
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
    assert_eq!(post.author_picture_url, "https://example.com/a.png");
    assert_eq!(post.author_picture_url, post.author_display.picture_url);
}
