use super::*;
use std::collections::HashMap;

fn parse(s: &str) -> Vec<MarkdownNode> {
    let blocks = parse_markdown_blocks(s, &HashMap::new());
    blocks
        .into_iter()
        .filter_map(|seg| {
            if let Segment::MarkdownBlock(b) = seg {
                Some(b)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn heading_parses_with_level() {
    let blocks = parse("# Hello");
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0], MarkdownNode::Heading { level: 1, .. }));
}

#[test]
fn paragraph_parses() {
    let blocks = parse("hello world");
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0], MarkdownNode::Paragraph(_)));
}

#[test]
fn fenced_code_block_preserved_verbatim() {
    let blocks = parse("```rust\nlet x = 1;\n```");
    if let MarkdownNode::CodeBlock { info, body } = &blocks[0] {
        assert_eq!(info.as_deref(), Some("rust"));
        assert!(body.contains("let x = 1;"));
    } else {
        panic!("expected code block, got {:?}", blocks[0]);
    }
}

#[test]
fn rule_emits_rule_node() {
    let blocks = parse("---");
    assert!(matches!(blocks[0], MarkdownNode::Rule));
}

#[test]
fn hashtag_inside_paragraph_routes_through_inline_tokenizer() {
    let blocks = parse("hello #nostr world");
    let MarkdownNode::Paragraph(inlines) = &blocks[0] else {
        panic!("expected paragraph");
    };
    assert!(inlines.iter().any(|i| matches!(i,
        MarkdownInline::Inline(Segment::Hashtag(h)) if h == "nostr"
    )));
}

#[test]
fn bold_and_italic_wrap_inline_children() {
    let blocks = parse("**bold** and *italic*");
    let MarkdownNode::Paragraph(inlines) = &blocks[0] else {
        panic!("expected paragraph");
    };
    assert!(inlines
        .iter()
        .any(|i| matches!(i, MarkdownInline::Strong(_))));
    assert!(inlines
        .iter()
        .any(|i| matches!(i, MarkdownInline::Emphasis(_))));
}

#[test]
fn link_emits_link_inline() {
    let blocks = parse("[label](https://x.test/)");
    let MarkdownNode::Paragraph(inlines) = &blocks[0] else {
        panic!("expected paragraph");
    };
    assert!(inlines
        .iter()
        .any(|i| matches!(i, MarkdownInline::Link { .. })));
}

#[test]
fn bullet_list_emits_items_with_paragraphs() {
    let blocks = parse("- one\n- two\n");
    let MarkdownNode::List {
        ordered_start,
        items,
    } = &blocks[0]
    else {
        panic!("expected list");
    };
    assert!(ordered_start.is_none());
    assert_eq!(items.len(), 2);
}

fn find_image(inlines: &[MarkdownInline]) -> Option<(&str, Option<&str>)> {
    inlines.iter().find_map(|i| match i {
        MarkdownInline::Image { alt, title, .. } => Some((alt.as_str(), title.as_deref())),
        _ => None,
    })
}

#[test]
fn image_uses_real_alt_text_not_title() {
    let blocks = parse(r#"![real alt](https://x.test/i.png "the title")"#);
    let MarkdownNode::Paragraph(inlines) = &blocks[0] else {
        panic!("expected paragraph, got {:?}", blocks[0]);
    };
    let (alt, title) = find_image(inlines).expect("image inline");
    assert_eq!(alt, "real alt");
    assert_eq!(title, Some("the title"));
}

#[test]
fn image_alt_does_not_leak_as_inline_text() {
    let blocks = parse("![alt words](https://x.test/i.png)");
    let MarkdownNode::Paragraph(inlines) = &blocks[0] else {
        panic!("expected paragraph");
    };
    // The alt must NOT appear as a sibling Text inline.
    let leaked = inlines
        .iter()
        .any(|i| matches!(i, MarkdownInline::Inline(Segment::Text(t)) if t.contains("alt words")));
    assert!(!leaked, "alt text leaked as inline: {inlines:?}");
    let (alt, title) = find_image(inlines).expect("image inline");
    assert_eq!(alt, "alt words");
    assert_eq!(title, None);
}

#[test]
fn gfm_table_not_parsed_as_table_pd012() {
    // PD-012: tables are NOT a CommonMark feature; pulldown must treat
    // the pipes as literal paragraph text, not a Table node.
    let blocks = parse("| a | b |\n|---|---|\n| 1 | 2 |");
    assert!(
        blocks.iter().all(|b| !matches!(b, MarkdownNode::Rule)),
        "table separator must not become a Rule"
    );
    assert!(matches!(blocks[0], MarkdownNode::Paragraph(_)));
}

#[test]
fn gfm_strikethrough_not_parsed_pd012() {
    // `~~x~~` stays literal text under CommonMark-only options.
    let blocks = parse("~~struck~~");
    let MarkdownNode::Paragraph(inlines) = &blocks[0] else {
        panic!("expected paragraph");
    };
    let text: String = inlines
        .iter()
        .filter_map(|i| match i {
            MarkdownInline::Inline(Segment::Text(t)) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        text.contains("~~"),
        "tildes must remain literal: {inlines:?}"
    );
}

#[test]
fn ordered_list_carries_start() {
    let blocks = parse("1. one\n2. two\n");
    let MarkdownNode::List {
        ordered_start,
        items,
    } = &blocks[0]
    else {
        panic!("expected list");
    };
    assert_eq!(*ordered_start, Some(1));
    assert_eq!(items.len(), 2);
}
