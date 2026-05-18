//! 1:1 projection from the real `nmp_content` IR to serde DTOs.
//!
//! These `From` impls are the *consumption* of `nmp-content`: we import its
//! public types unchanged and translate the tree the **real**
//! `tokenize_with_kind` produced. No `nmp-content` source is edited.

use nmp_content::{
    ContentTree, InvoiceKind, MarkdownInline, MarkdownNode, MediaKind, Segment,
};
use nmp_core::nip21::{format_nostr_uri, NostrUri};

use crate::dto::{
    ContentTreeDto, MarkdownInlineDto, MarkdownNodeDto, SegmentDto,
};

fn uri_string(u: &NostrUri) -> String {
    format_nostr_uri(u).unwrap_or_else(|_| "nostr:invalid".to_string())
}

fn project_mention(u: &NostrUri) -> SegmentDto {
    match u {
        NostrUri::Profile { pubkey, relays } => SegmentDto::Mention {
            uri: uri_string(u),
            kind: if relays.is_empty() { "npub" } else { "nprofile" }
                .to_string(),
            pubkey: pubkey.clone(),
        },
        // The tokenizer only routes Profile entities to `Segment::Mention`.
        other => SegmentDto::Text {
            text: uri_string(other),
        },
    }
}

fn project_event_ref(u: &NostrUri) -> SegmentDto {
    match u {
        NostrUri::Event { event_id, relays, author, kind } => {
            let is_note =
                relays.is_empty() && author.is_none() && kind.is_none();
            SegmentDto::EventRef {
                uri: uri_string(u),
                kind: if is_note { "note" } else { "nevent" }.to_string(),
                id: event_id.clone(),
            }
        }
        NostrUri::Address { identifier, pubkey, kind, .. } => {
            SegmentDto::EventRef {
                uri: uri_string(u),
                kind: "naddr".to_string(),
                id: format!("{kind}:{pubkey}:{identifier}"),
            }
        }
        other => SegmentDto::Text {
            text: uri_string(other),
        },
    }
}

fn media_kind(k: &MediaKind) -> &'static str {
    match k {
        MediaKind::Image => "Image",
        MediaKind::Video => "Video",
        MediaKind::Audio => "Audio",
    }
}

fn project_segment(s: &Segment) -> SegmentDto {
    match s {
        Segment::Text(t) => SegmentDto::Text { text: t.clone() },
        Segment::Mention(u) => project_mention(u),
        Segment::EventRef(u) => project_event_ref(u),
        Segment::Hashtag(h) => SegmentDto::Hashtag { tag: h.clone() },
        Segment::Url(u) => SegmentDto::Url {
            url: u.to_string(),
        },
        Segment::Media { urls, kind } => SegmentDto::Media {
            media_kind: media_kind(kind).to_string(),
            urls: urls.iter().map(|u| u.to_string()).collect(),
        },
        Segment::Emoji { shortcode, url } => SegmentDto::Emoji {
            shortcode: shortcode.clone(),
            url: url.as_ref().map(|u| u.to_string()),
        },
        Segment::Invoice(kind) => {
            let (k, v) = match kind {
                InvoiceKind::Bolt11(s) => ("Bolt11", s.clone()),
                InvoiceKind::Bolt12(s) => ("Bolt12", s.clone()),
                InvoiceKind::Cashu(s) => ("Cashu", s.clone()),
            };
            SegmentDto::Invoice {
                invoice_kind: k.to_string(),
                value: v,
            }
        }
        Segment::MarkdownBlock(node) => SegmentDto::MarkdownBlock {
            node: project_node(node),
        },
    }
}

fn project_inline(i: &MarkdownInline) -> MarkdownInlineDto {
    match i {
        MarkdownInline::Inline(seg) => MarkdownInlineDto::Inline {
            segment: project_segment(seg),
        },
        MarkdownInline::Emphasis(c) => MarkdownInlineDto::Emphasis {
            children: c.iter().map(project_inline).collect(),
        },
        MarkdownInline::Strong(c) => MarkdownInlineDto::Strong {
            children: c.iter().map(project_inline).collect(),
        },
        MarkdownInline::Code(t) => MarkdownInlineDto::Code {
            text: t.clone(),
        },
        MarkdownInline::Link { label, href } => MarkdownInlineDto::Link {
            label: label.iter().map(project_inline).collect(),
            href: href.as_ref().map(|u| u.to_string()),
        },
        MarkdownInline::Image { alt, title, src } => {
            MarkdownInlineDto::Image {
                alt: alt.clone(),
                title: title.clone(),
                src: src.as_ref().map(|u| u.to_string()),
            }
        }
        MarkdownInline::SoftBreak => MarkdownInlineDto::SoftBreak,
        MarkdownInline::HardBreak => MarkdownInlineDto::HardBreak,
    }
}

fn project_node(n: &MarkdownNode) -> MarkdownNodeDto {
    match n {
        MarkdownNode::Heading { level, inlines } => {
            MarkdownNodeDto::Heading {
                level: *level,
                inlines: inlines.iter().map(project_inline).collect(),
            }
        }
        MarkdownNode::Paragraph(inlines) => MarkdownNodeDto::Paragraph {
            inlines: inlines.iter().map(project_inline).collect(),
        },
        MarkdownNode::BlockQuote(blocks) => MarkdownNodeDto::BlockQuote {
            blocks: blocks.iter().map(project_node).collect(),
        },
        MarkdownNode::CodeBlock { info, body } => {
            MarkdownNodeDto::CodeBlock {
                info: info.clone(),
                body: body.clone(),
            }
        }
        MarkdownNode::List { ordered_start, items } => {
            MarkdownNodeDto::List {
                ordered_start: *ordered_start,
                items: items
                    .iter()
                    .map(|item| {
                        item.iter().map(project_node).collect()
                    })
                    .collect(),
            }
        }
        MarkdownNode::Rule => MarkdownNodeDto::Rule,
    }
}

/// Project a real `ContentTree` (output of `nmp_content::tokenize_with_kind`)
/// to the serde DTO mirror.
pub fn project_tree(tree: &ContentTree) -> ContentTreeDto {
    ContentTreeDto {
        mode: format!("{:?}", tree.mode),
        segments: tree.segments.iter().map(project_segment).collect(),
    }
}
