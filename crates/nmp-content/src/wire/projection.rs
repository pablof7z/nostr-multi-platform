//! Pure projection of the internal recursive [`ContentTree`] into the flat,
//! serde-serializable [`ContentTreeWire`] arena. Honours D1 (unprojectable
//! content → typed [`WireNode::Placeholder`], never dropped) and D6 (no
//! `unwrap`/`expect`/panicking-index on non-test paths).

use nmp_core::nip21::{format_nostr_uri, NostrUri};
use url::Url;

use crate::markdown::{MarkdownInline, MarkdownNode};
use crate::segment::{ContentTree, Segment};

use super::{
    ContentTreeWire, PlaceholderReason, WireNode, WireNostrUri, WireNostrUriKind, WIRE_MAX_DEPTH,
};

impl ContentTree {
    /// Project this internal tree into its flat, serde-serializable FFI wire
    /// form. Pure; allocates a fresh arena. Honours D1 (unprojectable content
    /// becomes a typed [`WireNode::Placeholder`], never dropped) and D6 (no
    /// panics).
    #[must_use]
    pub fn to_wire(&self) -> ContentTreeWire {
        let mut builder = WireBuilder::default();
        let roots = self
            .segments
            .iter()
            .map(|seg| builder.push_segment(seg, 0))
            .collect();
        ContentTreeWire {
            nodes: builder.nodes,
            roots,
            mode: self.mode,
        }
    }
}

/// Arena accumulator. Every `push_*` returns the index of the node it added so
/// parents can record child-index lists.
#[derive(Default)]
struct WireBuilder {
    nodes: Vec<WireNode>,
}

impl WireBuilder {
    fn push(&mut self, node: WireNode) -> u32 {
        // `len()` is the index the node will occupy. Capped well below u32::MAX
        // in practice; saturate defensively rather than risk a wrap (D6).
        let idx = u32::try_from(self.nodes.len()).unwrap_or(u32::MAX);
        self.nodes.push(node);
        idx
    }

    fn placeholder(&mut self, reason: PlaceholderReason) -> u32 {
        self.push(WireNode::Placeholder { reason })
    }

    /// Project one inline-or-block `Segment`. `depth` bounds arena nesting.
    fn push_segment(&mut self, seg: &Segment, depth: u32) -> u32 {
        if depth >= WIRE_MAX_DEPTH {
            return self.placeholder(PlaceholderReason::DepthLimit);
        }
        match seg {
            Segment::Text(t) => self.push(WireNode::Text { text: t.clone() }),
            Segment::Mention(uri) => match project_uri(uri) {
                Some(w) => self.push(WireNode::Mention { uri: w }),
                None => self.placeholder(PlaceholderReason::UnresolvedUri),
            },
            Segment::EventRef(uri) => match project_uri(uri) {
                Some(w) => self.push(WireNode::EventRef { uri: w }),
                None => self.placeholder(PlaceholderReason::UnresolvedUri),
            },
            Segment::Hashtag(h) => self.push(WireNode::Hashtag { tag: h.clone() }),
            Segment::Url(u) => self.push(WireNode::Url { url: u.to_string() }),
            Segment::Media { urls, kind } => self.push(WireNode::Media {
                urls: urls.iter().map(Url::to_string).collect(),
                media_kind: *kind,
            }),
            Segment::Emoji { shortcode, url } => self.push(WireNode::Emoji {
                shortcode: shortcode.clone(),
                url: url.as_ref().map(Url::to_string),
            }),
            Segment::Invoice(kind) => self.push(WireNode::Invoice {
                invoice: kind.clone(),
            }),
            Segment::MarkdownBlock(node) => self.push_block(node, depth),
        }
    }

    fn push_block(&mut self, node: &MarkdownNode, depth: u32) -> u32 {
        if depth >= WIRE_MAX_DEPTH {
            return self.placeholder(PlaceholderReason::DepthLimit);
        }
        match node {
            MarkdownNode::Heading { level, inlines } => {
                let children = self.push_inlines(inlines, depth + 1);
                self.push(WireNode::Heading {
                    level: *level,
                    children,
                })
            }
            MarkdownNode::Paragraph(inlines) => {
                let children = self.push_inlines(inlines, depth + 1);
                self.push(WireNode::Paragraph { children })
            }
            MarkdownNode::BlockQuote(blocks) => {
                let children = self.push_blocks(blocks, depth + 1);
                self.push(WireNode::BlockQuote { children })
            }
            MarkdownNode::CodeBlock { info, body } => self.push(WireNode::CodeBlock {
                info: info.clone(),
                body: body.clone(),
            }),
            MarkdownNode::List {
                ordered_start,
                items,
            } => {
                let items = items
                    .iter()
                    .map(|item| self.push_blocks(item, depth + 1))
                    .collect();
                self.push(WireNode::List {
                    ordered_start: *ordered_start,
                    items,
                })
            }
            MarkdownNode::Rule => self.push(WireNode::Rule),
        }
    }

    fn push_blocks(&mut self, blocks: &[MarkdownNode], depth: u32) -> Vec<u32> {
        if depth >= WIRE_MAX_DEPTH {
            return vec![self.placeholder(PlaceholderReason::DepthLimit)];
        }
        blocks.iter().map(|b| self.push_block(b, depth)).collect()
    }

    fn push_inlines(&mut self, inlines: &[MarkdownInline], depth: u32) -> Vec<u32> {
        if depth >= WIRE_MAX_DEPTH {
            return vec![self.placeholder(PlaceholderReason::DepthLimit)];
        }
        inlines.iter().map(|i| self.push_inline(i, depth)).collect()
    }

    fn push_inline(&mut self, inline: &MarkdownInline, depth: u32) -> u32 {
        if depth >= WIRE_MAX_DEPTH {
            return self.placeholder(PlaceholderReason::DepthLimit);
        }
        match inline {
            MarkdownInline::Inline(seg) => self.push_segment(seg, depth + 1),
            MarkdownInline::Emphasis(children) => {
                let children = self.push_inlines(children, depth + 1);
                self.push(WireNode::Emphasis { children })
            }
            MarkdownInline::Strong(children) => {
                let children = self.push_inlines(children, depth + 1);
                self.push(WireNode::Strong { children })
            }
            MarkdownInline::Code(code) => self.push(WireNode::InlineCode { code: code.clone() }),
            MarkdownInline::Link { label, href } => {
                let children = self.push_inlines(label, depth + 1);
                self.push(WireNode::Link {
                    children,
                    href: href.as_ref().map(Url::to_string),
                })
            }
            MarkdownInline::Image { alt, title, src } => self.push(WireNode::Image {
                alt: alt.clone(),
                title: title.clone(),
                src: src.as_ref().map(Url::to_string),
            }),
            MarkdownInline::SoftBreak => self.push(WireNode::SoftBreak),
            MarkdownInline::HardBreak => self.push(WireNode::HardBreak),
        }
    }
}

/// Flatten a `NostrUri` to its wire form. Returns `None` only if
/// `format_nostr_uri` fails (structurally should not happen, but D6 forbids a
/// panic) — the caller emits a typed placeholder, never drops the segment.
fn project_uri(uri: &NostrUri) -> Option<WireNostrUri> {
    let canonical = format_nostr_uri(uri).ok()?;
    let projected = match uri {
        NostrUri::Profile { pubkey, relays } => WireNostrUri {
            uri: canonical,
            kind: WireNostrUriKind::Profile,
            primary_id: pubkey.clone(),
            relays: relays.clone(),
            author: None,
            event_kind: None,
        },
        NostrUri::Event {
            event_id,
            relays,
            author,
            kind,
        } => WireNostrUri {
            uri: canonical,
            kind: WireNostrUriKind::Event,
            primary_id: event_id.clone(),
            relays: relays.clone(),
            author: author.clone(),
            event_kind: *kind,
        },
        NostrUri::Address {
            identifier,
            pubkey,
            kind,
            relays,
        } => WireNostrUri {
            uri: canonical,
            kind: WireNostrUriKind::Address,
            // Coordinate string `"{kind}:{pubkey}:{d_tag}"` matches the
            // kernel's `claimed_events[primary_id]` snapshot key, so the
            // renderer's `envelope_for(uri)` lookup hits without an extra
            // alias map on the host side. (Previously `pubkey.clone()`,
            // which was ambiguous — the same author can have many
            // addressable events under different d-tags.)
            primary_id: format!("{kind}:{pubkey}:{identifier}"),
            relays: relays.clone(),
            author: Some(pubkey.clone()),
            event_kind: Some(*kind),
        },
    };
    Some(projected)
}
