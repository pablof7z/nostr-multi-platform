//! Markdown ADT + the `RenderMode::Markdown` render path.
//!
//! **One parser, two render paths** invariant (§10 #3): markdown blocks
//! recursively contain the same inline `Segment` variants as plaintext
//! mode. We do NOT fork the inline tokenizer — the inline runs inside a
//! block delegate to [`crate::tokenizer::tokenize_inline`].
//!
//! `pulldown-cmark` is an implementation detail; its types are kept out of
//! the public API per §10 #8. Only the [`MarkdownNode`] / [`MarkdownInline`]
//! ADTs cross the boundary.

use std::collections::HashMap;

use pulldown_cmark::{
    CodeBlockKind, Event as MdEvent, HeadingLevel, Options, Parser, Tag, TagEnd,
};
use url::Url;

use crate::segment::Segment;
use crate::tokenizer::tokenize_inline;

/// Markdown block-level node. The variant set is deliberately small —
/// CommonMark-core only. GFM extensions are explicitly excluded per
/// `content-rendering.md` PD-012.
///
/// No serde derives — see [`crate::segment::Segment`] doc-comment for why.
#[derive(Clone, Debug, PartialEq)]
pub enum MarkdownNode {
    /// `# heading` — level 1-6.
    Heading {
        /// Heading level 1-6 (`#` count).
        level: u8,
        /// Inline runs comprising the heading.
        inlines: Vec<MarkdownInline>,
    },
    /// Paragraph of inline runs.
    Paragraph(Vec<MarkdownInline>),
    /// Block quote. Body is a list of nested blocks (commonly Paragraphs).
    BlockQuote(Vec<MarkdownNode>),
    /// Fenced/indented code block. `info` is the optional language token.
    CodeBlock {
        /// Optional info string (e.g. `rust`, `text`); `None` for indented
        /// code blocks or fences with no language.
        info: Option<String>,
        /// Raw code body — never tokenized for inline segments.
        body: String,
    },
    /// Bullet (`-`, `*`) or ordered list. `ordered_start` is `Some(n)` for
    /// ordered lists starting at `n`, `None` for bullet lists.
    List {
        /// Ordered start number, or `None` for bullet.
        ordered_start: Option<u64>,
        /// Each item is a list of nested blocks (paragraph + optional
        /// sub-list etc.).
        items: Vec<Vec<MarkdownNode>>,
    },
    /// Horizontal rule (`---`, `***`).
    Rule,
}

/// Inline-level run inside a markdown block. Wraps `Segment` to add the
/// markdown-specific emphasis/code/link/image wrappers without forking the
/// inline IR.
#[derive(Clone, Debug, PartialEq)]
pub enum MarkdownInline {
    /// One of the plaintext inline `Segment` variants (Text, Mention,
    /// EventRef, Hashtag, Url, Media, Emoji, Invoice). MarkdownBlock is
    /// never emitted here (blocks don't nest inline).
    Inline(Segment),
    /// `*italic*` or `_italic_` — children are themselves inline runs.
    Emphasis(Vec<MarkdownInline>),
    /// `**bold**` or `__bold__`.
    Strong(Vec<MarkdownInline>),
    /// `` `code` `` — raw text, never tokenized for inline segments.
    Code(String),
    /// `[label](href)` — `label` is itself inline.
    Link {
        /// Display label (markdown inline runs — may contain other tokens).
        label: Vec<MarkdownInline>,
        /// Destination URL (validated; falls back to `None` if unparseable).
        href: Option<Url>,
    },
    /// `![alt](src)`.
    Image {
        /// Image alt text.
        alt: String,
        /// Source URL; `None` if unparseable.
        src: Option<Url>,
    },
    /// Soft line break (single `\n`).
    SoftBreak,
    /// Hard line break (`\\n` or two trailing spaces).
    HardBreak,
}

/// Parse markdown `content` into a flat sequence of `Segment::MarkdownBlock`
/// entries. Inline runs inside blocks delegate to the plaintext tokenizer.
pub(crate) fn parse_markdown_blocks(
    content: &str,
    emoji_table: &HashMap<String, Url>,
) -> Vec<Segment> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(content, options);
    let mut walker = Walker::new(emoji_table);
    for event in parser {
        walker.handle(event);
    }
    walker
        .finish()
        .into_iter()
        .map(Segment::MarkdownBlock)
        .collect()
}

/// State machine that walks `Event` stream → `Vec<MarkdownNode>`.
///
/// `block_stack` holds in-progress block frames (BlockQuote, List, Item);
/// `inline_stack` holds in-progress inline frames (Emphasis, Strong, Link).
/// `pending_inlines` accumulates inline runs for the innermost block.
struct Walker<'a> {
    emoji_table: &'a HashMap<String, Url>,
    blocks: Vec<MarkdownNode>,
    block_stack: Vec<BlockFrame>,
    inline_stack: Vec<InlineFrame>,
    pending_inlines: Vec<MarkdownInline>,
    pending_code: Option<CodeFrame>,
}

struct CodeFrame {
    info: Option<String>,
    body: String,
}

enum BlockFrame {
    Heading { level: u8 },
    Paragraph,
    BlockQuote { body: Vec<MarkdownNode> },
    List { ordered_start: Option<u64>, items: Vec<Vec<MarkdownNode>> },
    Item { body: Vec<MarkdownNode> },
}

enum InlineFrame {
    Emphasis(Vec<MarkdownInline>),
    Strong(Vec<MarkdownInline>),
    Link { label: Vec<MarkdownInline>, href: Option<Url> },
}

impl<'a> Walker<'a> {
    fn new(emoji_table: &'a HashMap<String, Url>) -> Self {
        Self {
            emoji_table,
            blocks: Vec::new(),
            block_stack: Vec::new(),
            inline_stack: Vec::new(),
            pending_inlines: Vec::new(),
            pending_code: None,
        }
    }

    fn finish(self) -> Vec<MarkdownNode> {
        self.blocks
    }

    fn handle(&mut self, event: MdEvent<'_>) {
        match event {
            MdEvent::Start(tag) => self.start(tag),
            MdEvent::End(tag) => self.end(tag),
            MdEvent::Text(t) => self.push_text(&t),
            MdEvent::Code(c) => self.push_inline(MarkdownInline::Code(c.into_string())),
            MdEvent::SoftBreak => self.push_inline(MarkdownInline::SoftBreak),
            MdEvent::HardBreak => self.push_inline(MarkdownInline::HardBreak),
            MdEvent::Rule => self.emit_block(MarkdownNode::Rule),
            // Math/HTML/footnotes/tasks pass through as raw text. We do not
            // ship these features into the IR — apps can switch on
            // `MarkdownNode::Paragraph` and inspect inline segments.
            MdEvent::Html(h) | MdEvent::InlineHtml(h) => self.push_text(&h),
            MdEvent::InlineMath(m) | MdEvent::DisplayMath(m) => self.push_text(&m),
            MdEvent::FootnoteReference(_) | MdEvent::TaskListMarker(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.block_stack.push(BlockFrame::Heading { level: heading_level(level) });
            }
            Tag::Paragraph => self.block_stack.push(BlockFrame::Paragraph),
            Tag::BlockQuote(_) => self.block_stack.push(BlockFrame::BlockQuote { body: Vec::new() }),
            Tag::List(start) => self
                .block_stack
                .push(BlockFrame::List { ordered_start: start, items: Vec::new() }),
            Tag::Item => self.block_stack.push(BlockFrame::Item { body: Vec::new() }),
            Tag::CodeBlock(kind) => {
                let info = match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => Some(lang.into_string()),
                    _ => None,
                };
                self.pending_code = Some(CodeFrame { info, body: String::new() });
            }
            Tag::Emphasis => self.inline_stack.push(InlineFrame::Emphasis(Vec::new())),
            Tag::Strong => self.inline_stack.push(InlineFrame::Strong(Vec::new())),
            Tag::Link { dest_url, .. } => self.inline_stack.push(InlineFrame::Link {
                label: Vec::new(),
                href: Url::parse(&dest_url).ok(),
            }),
            Tag::Image { dest_url, title, .. } => {
                self.push_inline(MarkdownInline::Image {
                    alt: title.into_string(),
                    src: Url::parse(&dest_url).ok(),
                });
            }
            // Tables, definition lists, footnotes, HTML blocks, math, sub/sup
            // pass through as paragraph-equivalents. The inner inline events
            // still arrive and accumulate into `pending_inlines`, then flush
            // on a synthetic Paragraph close. We model HtmlBlock as Paragraph.
            Tag::HtmlBlock | Tag::Table(_) | Tag::TableHead | Tag::TableRow | Tag::TableCell => {
                self.block_stack.push(BlockFrame::Paragraph);
            }
            Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                if let Some(BlockFrame::Heading { level }) = self.block_stack.pop() {
                    let inlines = std::mem::take(&mut self.pending_inlines);
                    self.emit_block(MarkdownNode::Heading { level, inlines });
                }
            }
            TagEnd::Paragraph => {
                if let Some(BlockFrame::Paragraph) = self.block_stack.pop() {
                    let inlines = std::mem::take(&mut self.pending_inlines);
                    if !inlines.is_empty() {
                        self.emit_block(MarkdownNode::Paragraph(inlines));
                    }
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(BlockFrame::BlockQuote { body }) = self.block_stack.pop() {
                    self.emit_block(MarkdownNode::BlockQuote(body));
                }
            }
            TagEnd::List(_) => {
                if let Some(BlockFrame::List { ordered_start, items }) = self.block_stack.pop() {
                    self.emit_block(MarkdownNode::List { ordered_start, items });
                }
            }
            TagEnd::Item => {
                if let Some(BlockFrame::Item { mut body }) = self.block_stack.pop() {
                    let inlines = std::mem::take(&mut self.pending_inlines);
                    if !inlines.is_empty() {
                        body.push(MarkdownNode::Paragraph(inlines));
                    }
                    if let Some(BlockFrame::List { items, .. }) = self.block_stack.last_mut() {
                        items.push(body);
                    }
                }
            }
            TagEnd::CodeBlock => {
                if let Some(frame) = self.pending_code.take() {
                    self.emit_block(MarkdownNode::CodeBlock { info: frame.info, body: frame.body });
                }
            }
            TagEnd::Emphasis => self.pop_inline_into(MarkdownInline::Emphasis),
            TagEnd::Strong => self.pop_inline_into(MarkdownInline::Strong),
            TagEnd::Link => {
                if let Some(InlineFrame::Link { label, href }) = self.inline_stack.pop() {
                    self.push_inline(MarkdownInline::Link { label, href });
                }
            }
            TagEnd::HtmlBlock | TagEnd::Table | TagEnd::TableHead | TagEnd::TableRow | TagEnd::TableCell => {
                if matches!(self.block_stack.last(), Some(BlockFrame::Paragraph)) {
                    let _ = self.block_stack.pop();
                    let inlines = std::mem::take(&mut self.pending_inlines);
                    if !inlines.is_empty() {
                        self.emit_block(MarkdownNode::Paragraph(inlines));
                    }
                }
            }
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        if let Some(frame) = self.pending_code.as_mut() {
            frame.body.push_str(text);
            return;
        }
        // Run the inline tokenizer on this text fragment so mentions/hashtags/
        // URLs that appear mid-paragraph still parse. Empty fragments after
        // tokenization (rare) are no-ops.
        let segments = tokenize_inline(text, self.emoji_table);
        for seg in segments {
            self.push_inline(MarkdownInline::Inline(seg));
        }
    }

    fn push_inline(&mut self, inline: MarkdownInline) {
        if let Some(frame) = self.inline_stack.last_mut() {
            match frame {
                InlineFrame::Emphasis(buf)
                | InlineFrame::Strong(buf)
                | InlineFrame::Link { label: buf, .. } => buf.push(inline),
            }
        } else {
            self.pending_inlines.push(inline);
        }
    }

    fn pop_inline_into(&mut self, wrap: impl FnOnce(Vec<MarkdownInline>) -> MarkdownInline) {
        let popped = match self.inline_stack.pop() {
            Some(InlineFrame::Emphasis(b)) | Some(InlineFrame::Strong(b)) => b,
            Some(InlineFrame::Link { label, href }) => {
                // Defensive: shouldn't happen — TagEnd::Link is handled
                // separately — but degrade gracefully if it does.
                self.push_inline(MarkdownInline::Link { label, href });
                return;
            }
            None => return,
        };
        self.push_inline(wrap(popped));
    }

    fn emit_block(&mut self, node: MarkdownNode) {
        if let Some(BlockFrame::BlockQuote { body }) | Some(BlockFrame::Item { body }) =
            self.block_stack.last_mut()
        {
            body.push(node);
        } else {
            self.blocks.push(node);
        }
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn parse(s: &str) -> Vec<MarkdownNode> {
        let blocks = parse_markdown_blocks(s, &HashMap::new());
        blocks
            .into_iter()
            .filter_map(|seg| if let Segment::MarkdownBlock(b) = seg { Some(b) } else { None })
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
        assert!(inlines.iter().any(|i| matches!(i, MarkdownInline::Strong(_))));
        assert!(inlines.iter().any(|i| matches!(i, MarkdownInline::Emphasis(_))));
    }

    #[test]
    fn link_emits_link_inline() {
        let blocks = parse("[label](https://x.test/)");
        let MarkdownNode::Paragraph(inlines) = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert!(inlines.iter().any(|i| matches!(i, MarkdownInline::Link { .. })));
    }

    #[test]
    fn bullet_list_emits_items_with_paragraphs() {
        let blocks = parse("- one\n- two\n");
        let MarkdownNode::List { ordered_start, items } = &blocks[0] else {
            panic!("expected list");
        };
        assert!(ordered_start.is_none());
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn ordered_list_carries_start() {
        let blocks = parse("1. one\n2. two\n");
        let MarkdownNode::List { ordered_start, items } = &blocks[0] else {
            panic!("expected list");
        };
        assert_eq!(*ordered_start, Some(1));
        assert_eq!(items.len(), 2);
    }
}
