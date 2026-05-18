//! ContentTree → wire → JSON → wire round-trip + depth-collapse tests.
//! See `docs/decisions/0018-content-tree-ffi-projection.md` (T93).

use nmp_content::{
    ContentTree, ContentTreeWire, MarkdownInline, MarkdownNode, PlaceholderReason,
    RenderMode, Segment, WireNode, WireNostrUriKind, WIRE_MAX_DEPTH,
};
use nmp_core::nip21::NostrUri;
use url::Url;

/// `wire -> json -> wire`. Tree itself is intentionally NOT round-tripped:
/// the internal tree has no `Deserialize` (that is the whole point of the
/// projection). Wire-level `PartialEq` after a JSON round trip is the contract.
fn json_round_trip(wire: &ContentTreeWire) -> ContentTreeWire {
    let json = serde_json::to_string(wire).expect("wire serialize");
    serde_json::from_str(&json).expect("wire deserialize")
}

#[test]
fn empty_tree_projects_to_empty_wire() {
    let wire = ContentTree::empty(RenderMode::Plain).to_wire();
    assert!(wire.nodes.is_empty());
    assert!(wire.roots.is_empty());
    assert_eq!(wire, json_round_trip(&wire));
}

#[test]
fn plain_segments_round_trip_tree_to_wire_to_json_to_wire() {
    let tree = ContentTree {
        segments: vec![
            Segment::Text("hi ".into()),
            Segment::Hashtag("nostr".into()),
            Segment::Url(Url::parse("https://x.test/a").unwrap()),
        ],
        mode: RenderMode::Plain,
    };
    let wire = tree.to_wire();
    assert_eq!(wire.roots, vec![0, 1, 2]);
    assert_eq!(wire, json_round_trip(&wire));
}

#[test]
fn mention_and_event_ref_project_with_discriminator_and_round_trip() {
    let tree = ContentTree {
        segments: vec![
            Segment::Mention(NostrUri::Profile {
                pubkey: "a".repeat(64),
                relays: vec![],
            }),
            Segment::EventRef(NostrUri::Event {
                event_id: "b".repeat(64),
                relays: vec!["wss://r.test".into()],
                author: Some("c".repeat(64)),
                kind: Some(1),
            }),
        ],
        mode: RenderMode::Plain,
    };
    let wire = tree.to_wire();
    match &wire.nodes[0] {
        WireNode::Mention { uri } => {
            assert_eq!(uri.kind, WireNostrUriKind::Profile);
            assert_eq!(uri.primary_id, "a".repeat(64));
            assert!(uri.uri.starts_with("nostr:"));
        }
        other => panic!("expected mention, got {other:?}"),
    }
    match &wire.nodes[1] {
        WireNode::EventRef { uri } => {
            assert_eq!(uri.kind, WireNostrUriKind::Event);
            assert_eq!(uri.event_kind, Some(1));
            assert_eq!(uri.author.as_deref(), Some("c".repeat(64).as_str()));
        }
        other => panic!("expected event ref, got {other:?}"),
    }
    assert_eq!(wire, json_round_trip(&wire));
}

#[test]
fn nested_markdown_flattens_to_index_arena_and_round_trips() {
    // BlockQuote > Paragraph > [Strong > [Text]]
    let tree = ContentTree {
        segments: vec![Segment::MarkdownBlock(MarkdownNode::BlockQuote(vec![
            MarkdownNode::Paragraph(vec![MarkdownInline::Strong(vec![
                MarkdownInline::Inline(Segment::Text("bold".into())),
            ])]),
        ]))],
        mode: RenderMode::Markdown,
    };
    let wire = tree.to_wire();
    assert_eq!(wire.roots.len(), 1);
    let root = wire.roots[0] as usize;
    assert!(matches!(wire.nodes[root], WireNode::BlockQuote { .. }));
    // Every parent→child edge is an index, so the JSON has no nested node
    // objects — round trip proves the flat shape is stable.
    assert_eq!(wire, json_round_trip(&wire));
}

#[test]
fn deeply_nested_blockquote_collapses_to_finite_placeholder() {
    // Safe-Rust analogue of a "cyclic" tree: an adversarial / recursion-
    // collapsed structure nested far past WIRE_MAX_DEPTH must still project
    // to a FINITE wire form carrying a typed DepthLimit placeholder
    // (D1: never dropped, D6: never panics).
    let mut node = MarkdownNode::Paragraph(vec![MarkdownInline::Inline(
        Segment::Text("deep".into()),
    )]);
    for _ in 0..(WIRE_MAX_DEPTH + 50) {
        node = MarkdownNode::BlockQuote(vec![node]);
    }
    let tree = ContentTree {
        segments: vec![Segment::MarkdownBlock(node)],
        mode: RenderMode::Markdown,
    };
    let wire = tree.to_wire();

    // Finite: bounded by the depth cap, not the (much larger) input nesting.
    assert!(
        (wire.nodes.len() as u32) <= WIRE_MAX_DEPTH + 2,
        "wire must be finite/bounded, got {} nodes",
        wire.nodes.len()
    );
    assert!(
        wire.nodes.iter().any(|n| matches!(
            n,
            WireNode::Placeholder {
                reason: PlaceholderReason::DepthLimit
            }
        )),
        "expected a DepthLimit placeholder node"
    );
    assert_eq!(wire, json_round_trip(&wire));
}

#[test]
fn json_shape_is_flat_arena_not_recursive() {
    let tree = ContentTree {
        segments: vec![Segment::MarkdownBlock(MarkdownNode::Paragraph(vec![
            MarkdownInline::Inline(Segment::Text("x".into())),
        ]))],
        mode: RenderMode::Markdown,
    };
    let json = serde_json::to_value(tree.to_wire()).unwrap();
    let obj = json.as_object().unwrap();
    assert!(obj.contains_key("nodes"));
    assert!(obj.contains_key("roots"));
    assert!(obj.contains_key("mode"));
    assert!(json["nodes"].is_array());
}

#[test]
fn all_inline_and_block_kinds_round_trip() {
    let tree = ContentTree {
        segments: vec![
            Segment::MarkdownBlock(MarkdownNode::Heading {
                level: 2,
                inlines: vec![MarkdownInline::Emphasis(vec![
                    MarkdownInline::Inline(Segment::Text("h".into())),
                ])],
            }),
            Segment::MarkdownBlock(MarkdownNode::CodeBlock {
                info: Some("rust".into()),
                body: "let x = 1;".into(),
            }),
            Segment::MarkdownBlock(MarkdownNode::List {
                ordered_start: Some(3),
                items: vec![vec![MarkdownNode::Paragraph(vec![
                    MarkdownInline::Link {
                        label: vec![MarkdownInline::Inline(Segment::Text("l".into()))],
                        href: Url::parse("https://x.test/").ok(),
                    },
                ])]],
            }),
            Segment::MarkdownBlock(MarkdownNode::Rule),
        ],
        mode: RenderMode::Markdown,
    };
    let wire = tree.to_wire();
    assert_eq!(wire, json_round_trip(&wire));
}
