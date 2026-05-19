//! Integration tests for `RenderMode::Auto` kind-sniffing — the cross-cut
//! contract that bridges the tokenizer with `sniff_mode_from_kind`.

use nmp_content::{sniff_mode_from_kind, tokenize_with_kind, MarkdownNode, RenderMode, Segment};

#[test]
fn kind_30023_auto_sniffs_to_markdown_and_parses_headings() {
    let tree = tokenize_with_kind("# Hello\n\nbody", &[], RenderMode::Auto, 30023);
    assert_eq!(tree.mode, RenderMode::Markdown);
    // First segment is the heading block.
    let Segment::MarkdownBlock(MarkdownNode::Heading { level, .. }) = &tree.segments[0] else {
        panic!("expected heading, got {:?}", tree.segments[0]);
    };
    assert_eq!(*level, 1);
}

#[test]
fn kind_30024_long_form_draft_sniffs_to_markdown() {
    let tree = tokenize_with_kind("## H2", &[], RenderMode::Auto, 30024);
    assert_eq!(tree.mode, RenderMode::Markdown);
}

#[test]
fn kind_30818_wiki_sniffs_to_markdown() {
    let tree = tokenize_with_kind("# Wiki", &[], RenderMode::Auto, 30818);
    assert_eq!(tree.mode, RenderMode::Markdown);
}

#[test]
fn kind_1_short_note_sniffs_to_plain_and_keeps_hash_literal() {
    let tree = tokenize_with_kind("# not heading", &[], RenderMode::Auto, 1);
    assert_eq!(tree.mode, RenderMode::Plain);
    // In plain mode, `# ` is literal text — not a heading.
    assert!(tree
        .segments
        .iter()
        .any(|s| matches!(s, Segment::Text(t) if t.contains("# not heading"))));
}

#[test]
fn kind_6_repost_sniffs_to_plain() {
    assert_eq!(sniff_mode_from_kind(6), RenderMode::Plain);
}

#[test]
fn explicit_markdown_overrides_kind_hint() {
    // Even though kind 1 would sniff to Plain, explicit Markdown wins.
    let tree = tokenize_with_kind("# heading", &[], RenderMode::Markdown, 1);
    assert_eq!(tree.mode, RenderMode::Markdown);
}

#[test]
fn explicit_plain_overrides_kind_hint() {
    let tree = tokenize_with_kind("# heading", &[], RenderMode::Plain, 30023);
    assert_eq!(tree.mode, RenderMode::Plain);
}

#[test]
fn auto_in_markdown_mode_still_tokenizes_inline_hashtags() {
    let tree = tokenize_with_kind("# heading\n\nbody #tag here", &[], RenderMode::Auto, 30023);
    assert_eq!(tree.mode, RenderMode::Markdown);
    let mut has_hashtag = false;
    for seg in &tree.segments {
        if let Segment::MarkdownBlock(MarkdownNode::Paragraph(inlines)) = seg {
            for inline in inlines {
                if let nmp_content::MarkdownInline::Inline(Segment::Hashtag(t)) = inline {
                    if t == "tag" {
                        has_hashtag = true;
                    }
                }
            }
        }
    }
    assert!(has_hashtag, "hashtag tokenization didn't propagate through markdown");
}

#[test]
fn auto_in_plain_mode_renders_hashtag_as_segment() {
    let tree = tokenize_with_kind("hello #tag", &[], RenderMode::Auto, 1);
    assert_eq!(tree.mode, RenderMode::Plain);
    assert!(tree
        .segments
        .iter()
        .any(|s| matches!(s, Segment::Hashtag(t) if t == "tag")));
}
