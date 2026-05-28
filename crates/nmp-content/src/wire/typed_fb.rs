//! Typed FlatBuffers wire codec for [`crate::wire::ContentTreeWire`].
//!
//! The canonical FFI shape is the serde JSON of [`ContentTreeWire`]
//! (`docs/decisions/0018-content-tree-ffi-projection.md`). This module adds a
//! **typed FlatBuffers** encoding of the same arena — a self-describing,
//! schema-versioned, language-neutral binary the host platforms (Swift /
//! Kotlin / TypeScript) can decode with generated accessors instead of JSON
//! reflection. It is a sidecar codec: the serde shape stays authoritative; this
//! is the typed payload carried in `TypedProjection` frames
//! (`crates/nmp-core/schema/nmp_update.fbs`).
//!
//! The schema (`crates/nmp-content/schema/content_tree.fbs`) uses the
//! optional-fields approach — a [`WireNodeKind`](generated::nmp::content::WireNodeKind)
//! discriminator plus optional payload fields on a single `WireNode` table —
//! rather than a union, for cross-platform stability. Decode dispatches purely
//! on `kind`; several variants share a field name (`text`, `children`), so the
//! discriminator is the only authority on which fields are meaningful.
//!
//! Scope: this codec encodes only the content tree. The `ContentRenderData`
//! embed-lookup sidecar lives in `nmp-nip01` and is encoded there — keeping
//! this Layer-A crate free of any back-edge into `nmp-nip01`.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input;
//! there are no `unwrap`/`expect`/panicking-index operations on the decode path.

// The generated FlatBuffers bindings are intrinsically `unsafe` (every accessor
// reads from a raw `Table`). The crate root relaxed `forbid(unsafe_code)` to
// `deny(unsafe_code)` so this single generated module — and only it — may opt
// back in. No hand-written code in this file uses `unsafe`.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "generated/content_tree_generated.rs"]
pub mod generated;

use generated::nmp::content as fb;

use crate::mode::RenderMode;
use crate::segment::{InvoiceKind, MediaKind};
use crate::wire::{ContentTreeWire, PlaceholderReason, WireNode, WireNostrUri, WireNostrUriKind};

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.content.tree";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NFCT";
/// Wire schema version. Bump on any breaking change to `content_tree.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

/// `ordered_start` sentinel for an unordered (bullet) list — see the schema.
const ORDERED_START_NONE: i64 = -1;

/// `event_kind` sentinel for `None`. The wire field is a non-optional `uint32`
/// (default 0), so `Some(0)` and `None` would otherwise be indistinguishable.
/// Real Nostr event kinds are `0..=65535` (NIP-01), so `u32::MAX` is a safe
/// "no kind" marker that round-trips `None` losslessly.
const EVENT_KIND_NONE: u32 = u32::MAX;

// --- enum bridges ---------------------------------------------------------

fn render_mode_to_fb(mode: RenderMode) -> fb::RenderMode {
    // The wire enum names "plain" as `Text` (value 2); `Markdown` is 1; `Auto`
    // is 0. `RenderMode::Plain` therefore maps to `fb::RenderMode::Text`.
    match mode {
        RenderMode::Auto => fb::RenderMode::Auto,
        RenderMode::Markdown => fb::RenderMode::Markdown,
        RenderMode::Plain => fb::RenderMode::Text,
    }
}

fn render_mode_from_fb(mode: fb::RenderMode) -> Result<RenderMode, String> {
    match mode {
        fb::RenderMode::Auto => Ok(RenderMode::Auto),
        fb::RenderMode::Markdown => Ok(RenderMode::Markdown),
        fb::RenderMode::Text => Ok(RenderMode::Plain),
        other => Err(format!("unknown RenderMode discriminant {}", other.0)),
    }
}

fn uri_kind_to_fb(kind: WireNostrUriKind) -> fb::WireNostrUriKind {
    match kind {
        WireNostrUriKind::Profile => fb::WireNostrUriKind::Profile,
        WireNostrUriKind::Event => fb::WireNostrUriKind::Event,
        WireNostrUriKind::Address => fb::WireNostrUriKind::Address,
    }
}

fn uri_kind_from_fb(kind: fb::WireNostrUriKind) -> Result<WireNostrUriKind, String> {
    match kind {
        fb::WireNostrUriKind::Profile => Ok(WireNostrUriKind::Profile),
        fb::WireNostrUriKind::Event => Ok(WireNostrUriKind::Event),
        fb::WireNostrUriKind::Address => Ok(WireNostrUriKind::Address),
        other => Err(format!("unknown WireNostrUriKind discriminant {}", other.0)),
    }
}

fn placeholder_to_fb(reason: PlaceholderReason) -> fb::PlaceholderReason {
    match reason {
        PlaceholderReason::DepthLimit => fb::PlaceholderReason::DepthLimit,
        PlaceholderReason::UnresolvedUri => fb::PlaceholderReason::UnresolvedUri,
    }
}

fn placeholder_from_fb(reason: fb::PlaceholderReason) -> Result<PlaceholderReason, String> {
    match reason {
        fb::PlaceholderReason::DepthLimit => Ok(PlaceholderReason::DepthLimit),
        fb::PlaceholderReason::UnresolvedUri => Ok(PlaceholderReason::UnresolvedUri),
        other => Err(format!(
            "unknown PlaceholderReason discriminant {}",
            other.0
        )),
    }
}

/// Declaration-order discriminant of [`MediaKind`] (Image=0, Video=1, Audio=2).
fn media_kind_to_u8(kind: MediaKind) -> u8 {
    match kind {
        MediaKind::Image => 0,
        MediaKind::Video => 1,
        MediaKind::Audio => 2,
    }
}

fn media_kind_from_u8(v: u8) -> Result<MediaKind, String> {
    match v {
        0 => Ok(MediaKind::Image),
        1 => Ok(MediaKind::Video),
        2 => Ok(MediaKind::Audio),
        other => Err(format!("unknown MediaKind discriminant {other}")),
    }
}

/// Discriminant of [`InvoiceKind`] (Bolt11=0, Bolt12=1, Cashu=2).
fn invoice_parts(invoice: &InvoiceKind) -> (u8, &str) {
    match invoice {
        InvoiceKind::Bolt11(s) => (0, s.as_str()),
        InvoiceKind::Bolt12(s) => (1, s.as_str()),
        InvoiceKind::Cashu(s) => (2, s.as_str()),
    }
}

fn invoice_from_parts(kind: u8, payload: &str) -> Result<InvoiceKind, String> {
    match kind {
        0 => Ok(InvoiceKind::Bolt11(payload.to_string())),
        1 => Ok(InvoiceKind::Bolt12(payload.to_string())),
        2 => Ok(InvoiceKind::Cashu(payload.to_string())),
        other => Err(format!("unknown InvoiceKind discriminant {other}")),
    }
}

// --- encode ---------------------------------------------------------------

/// Encode a [`ContentTreeWire`] to typed FlatBuffers bytes (with the `NFCT`
/// file identifier).
#[must_use]
pub fn encode_content_tree(tree: &ContentTreeWire) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    let node_offsets: Vec<flatbuffers::WIPOffset<fb::WireNode>> = tree
        .nodes
        .iter()
        .map(|node| encode_node(&mut fbb, node))
        .collect();
    let nodes_vec = fbb.create_vector(&node_offsets);
    let roots_vec = fbb.create_vector(&tree.roots);

    let root = fb::ContentTreeWire::create(
        &mut fbb,
        &fb::ContentTreeWireArgs {
            nodes: Some(nodes_vec),
            roots: Some(roots_vec),
            mode: render_mode_to_fb(tree.mode),
        },
    );
    fb::finish_content_tree_wire_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_node<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    node: &WireNode,
) -> flatbuffers::WIPOffset<fb::WireNode<'a>> {
    // All child offsets (strings, vectors, sub-tables) must be created before
    // the `WireNode` table is started, so build them up front into `args`.
    let mut args = fb::WireNodeArgs::default();

    match node {
        WireNode::Text { text } => {
            args.kind = fb::WireNodeKind::Text;
            args.text = Some(fbb.create_string(text));
        }
        WireNode::Mention { uri } => {
            args.kind = fb::WireNodeKind::Mention;
            args.nostr_uri = Some(encode_uri(fbb, uri));
        }
        WireNode::EventRef { uri } => {
            args.kind = fb::WireNodeKind::EventRef;
            args.nostr_uri = Some(encode_uri(fbb, uri));
        }
        WireNode::Hashtag { tag } => {
            args.kind = fb::WireNodeKind::Hashtag;
            args.tag = Some(fbb.create_string(tag));
        }
        WireNode::Url { url } => {
            args.kind = fb::WireNodeKind::Url;
            args.url = Some(fbb.create_string(url));
        }
        WireNode::Media { urls, media_kind } => {
            args.kind = fb::WireNodeKind::Media;
            let url_offsets: Vec<_> = urls.iter().map(|u| fbb.create_string(u)).collect();
            args.media_urls = Some(fbb.create_vector(&url_offsets));
            args.media_kind = media_kind_to_u8(*media_kind);
        }
        WireNode::Emoji { shortcode, url } => {
            args.kind = fb::WireNodeKind::Emoji;
            args.shortcode = Some(fbb.create_string(shortcode));
            args.emoji_url = url.as_ref().map(|u| fbb.create_string(u));
        }
        WireNode::Invoice { invoice } => {
            args.kind = fb::WireNodeKind::Invoice;
            let (disc, payload) = invoice_parts(invoice);
            args.invoice_kind = disc;
            args.invoice_payload = Some(fbb.create_string(payload));
        }
        WireNode::Heading { level, children } => {
            args.kind = fb::WireNodeKind::Heading;
            args.level = *level;
            args.children = Some(fbb.create_vector(children));
        }
        WireNode::Paragraph { children } => {
            args.kind = fb::WireNodeKind::Paragraph;
            args.children = Some(fbb.create_vector(children));
        }
        WireNode::BlockQuote { children } => {
            args.kind = fb::WireNodeKind::BlockQuote;
            args.children = Some(fbb.create_vector(children));
        }
        WireNode::CodeBlock { info, body } => {
            args.kind = fb::WireNodeKind::CodeBlock;
            args.text = Some(fbb.create_string(body));
            args.code_info = info.as_ref().map(|i| fbb.create_string(i));
        }
        WireNode::List {
            ordered_start,
            items,
        } => {
            args.kind = fb::WireNodeKind::List;
            let item_offsets: Vec<_> = items
                .iter()
                .map(|item_children| {
                    let children = fbb.create_vector(item_children);
                    fb::ListItem::create(
                        fbb,
                        &fb::ListItemArgs {
                            children: Some(children),
                        },
                    )
                })
                .collect();
            args.list_items = Some(fbb.create_vector(&item_offsets));
            args.ordered_start = match ordered_start {
                Some(n) => *n as i64,
                None => ORDERED_START_NONE,
            };
        }
        WireNode::Rule => {
            args.kind = fb::WireNodeKind::Rule;
        }
        WireNode::Emphasis { children } => {
            args.kind = fb::WireNodeKind::Emphasis;
            args.children = Some(fbb.create_vector(children));
        }
        WireNode::Strong { children } => {
            args.kind = fb::WireNodeKind::Strong;
            args.children = Some(fbb.create_vector(children));
        }
        WireNode::InlineCode { code } => {
            args.kind = fb::WireNodeKind::InlineCode;
            args.text = Some(fbb.create_string(code));
        }
        WireNode::Link { children, href } => {
            args.kind = fb::WireNodeKind::Link;
            args.children = Some(fbb.create_vector(children));
            args.href = href.as_ref().map(|h| fbb.create_string(h));
        }
        WireNode::Image { alt, title, src } => {
            args.kind = fb::WireNodeKind::Image;
            args.alt = Some(fbb.create_string(alt));
            args.img_title = title.as_ref().map(|t| fbb.create_string(t));
            args.url = src.as_ref().map(|s| fbb.create_string(s));
        }
        WireNode::SoftBreak => {
            args.kind = fb::WireNodeKind::SoftBreak;
        }
        WireNode::HardBreak => {
            args.kind = fb::WireNodeKind::HardBreak;
        }
        WireNode::Placeholder { reason } => {
            args.kind = fb::WireNodeKind::Placeholder;
            args.placeholder_reason = placeholder_to_fb(*reason);
        }
    }

    fb::WireNode::create(fbb, &args)
}

fn encode_uri<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    uri: &WireNostrUri,
) -> flatbuffers::WIPOffset<fb::WireNostrUri<'a>> {
    let uri_str = fbb.create_string(&uri.uri);
    let primary_id = fbb.create_string(&uri.primary_id);
    let relay_offsets: Vec<_> = uri.relays.iter().map(|r| fbb.create_string(r)).collect();
    let relays = fbb.create_vector(&relay_offsets);
    let author = uri.author.as_ref().map(|a| fbb.create_string(a));

    fb::WireNostrUri::create(
        fbb,
        &fb::WireNostrUriArgs {
            uri: Some(uri_str),
            kind: uri_kind_to_fb(uri.kind),
            primary_id: Some(primary_id),
            relays: Some(relays),
            author,
            event_kind: uri.event_kind.unwrap_or(EVENT_KIND_NONE),
        },
    )
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_content_tree`]) back
/// into a [`ContentTreeWire`]. Returns an error string on any malformed input.
pub fn decode_content_tree(bytes: &[u8]) -> Result<ContentTreeWire, String> {
    let root = fb::root_as_content_tree_wire(bytes)
        .map_err(|e| format!("not a valid ContentTreeWire buffer: {e}"))?;

    let mode = render_mode_from_fb(root.mode())?;

    let mut nodes = Vec::new();
    if let Some(fb_nodes) = root.nodes() {
        nodes.reserve(fb_nodes.len());
        for fb_node in fb_nodes.iter() {
            nodes.push(decode_node(fb_node)?);
        }
    }

    let roots = match root.roots() {
        Some(r) => r.iter().collect(),
        None => Vec::new(),
    };

    Ok(ContentTreeWire { nodes, roots, mode })
}

fn decode_node(node: fb::WireNode) -> Result<WireNode, String> {
    let kind = node.kind();
    match kind {
        fb::WireNodeKind::Text => Ok(WireNode::Text {
            text: str_field(node.text(), "Text.text")?,
        }),
        fb::WireNodeKind::Mention => Ok(WireNode::Mention {
            uri: decode_uri(node.nostr_uri(), "Mention.nostr_uri")?,
        }),
        fb::WireNodeKind::EventRef => Ok(WireNode::EventRef {
            uri: decode_uri(node.nostr_uri(), "EventRef.nostr_uri")?,
        }),
        fb::WireNodeKind::Hashtag => Ok(WireNode::Hashtag {
            tag: str_field(node.tag(), "Hashtag.tag")?,
        }),
        fb::WireNodeKind::Url => Ok(WireNode::Url {
            url: str_field(node.url(), "Url.url")?,
        }),
        fb::WireNodeKind::Media => {
            let urls = match node.media_urls() {
                Some(v) => v.iter().map(str::to_string).collect(),
                None => Vec::new(),
            };
            Ok(WireNode::Media {
                urls,
                media_kind: media_kind_from_u8(node.media_kind())?,
            })
        }
        fb::WireNodeKind::Emoji => Ok(WireNode::Emoji {
            shortcode: str_field(node.shortcode(), "Emoji.shortcode")?,
            url: node.emoji_url().map(str::to_string),
        }),
        fb::WireNodeKind::Invoice => Ok(WireNode::Invoice {
            invoice: invoice_from_parts(node.invoice_kind(), node.invoice_payload().unwrap_or(""))?,
        }),
        fb::WireNodeKind::Heading => Ok(WireNode::Heading {
            level: node.level(),
            children: u32_vec(node.children()),
        }),
        fb::WireNodeKind::Paragraph => Ok(WireNode::Paragraph {
            children: u32_vec(node.children()),
        }),
        fb::WireNodeKind::BlockQuote => Ok(WireNode::BlockQuote {
            children: u32_vec(node.children()),
        }),
        fb::WireNodeKind::CodeBlock => Ok(WireNode::CodeBlock {
            info: node.code_info().map(str::to_string),
            body: str_field(node.text(), "CodeBlock.body")?,
        }),
        fb::WireNodeKind::List => {
            let items = match node.list_items() {
                Some(v) => v.iter().map(|item| u32_vec(item.children())).collect(),
                None => Vec::new(),
            };
            let ordered_start = match node.ordered_start() {
                ORDERED_START_NONE => None,
                n if n >= 0 => Some(n as u64),
                n => return Err(format!("invalid List.ordered_start {n}")),
            };
            Ok(WireNode::List {
                ordered_start,
                items,
            })
        }
        fb::WireNodeKind::Rule => Ok(WireNode::Rule),
        fb::WireNodeKind::Emphasis => Ok(WireNode::Emphasis {
            children: u32_vec(node.children()),
        }),
        fb::WireNodeKind::Strong => Ok(WireNode::Strong {
            children: u32_vec(node.children()),
        }),
        fb::WireNodeKind::InlineCode => Ok(WireNode::InlineCode {
            code: str_field(node.text(), "InlineCode.code")?,
        }),
        fb::WireNodeKind::Link => Ok(WireNode::Link {
            children: u32_vec(node.children()),
            href: node.href().map(str::to_string),
        }),
        fb::WireNodeKind::Image => Ok(WireNode::Image {
            alt: str_field(node.alt(), "Image.alt")?,
            title: node.img_title().map(str::to_string),
            src: node.url().map(str::to_string),
        }),
        fb::WireNodeKind::SoftBreak => Ok(WireNode::SoftBreak),
        fb::WireNodeKind::HardBreak => Ok(WireNode::HardBreak),
        fb::WireNodeKind::Placeholder => Ok(WireNode::Placeholder {
            reason: placeholder_from_fb(node.placeholder_reason())?,
        }),
        other => Err(format!("unknown WireNodeKind discriminant {}", other.0)),
    }
}

fn decode_uri(uri: Option<fb::WireNostrUri>, ctx: &str) -> Result<WireNostrUri, String> {
    let uri = uri.ok_or_else(|| format!("{ctx}: missing nostr_uri table"))?;
    Ok(WireNostrUri {
        uri: str_field(uri.uri(), "WireNostrUri.uri")?,
        kind: uri_kind_from_fb(uri.kind())?,
        primary_id: str_field(uri.primary_id(), "WireNostrUri.primary_id")?,
        relays: match uri.relays() {
            Some(v) => v.iter().map(str::to_string).collect(),
            None => Vec::new(),
        },
        author: uri.author().map(str::to_string),
        // `event_kind` uses `EVENT_KIND_NONE` (`u32::MAX`) as the `None` marker
        // so `Some(0)` round-trips distinctly from `None`. Real Nostr kinds are
        // `0..=65535`, so the sentinel never collides with a genuine value.
        event_kind: match uri.event_kind() {
            EVENT_KIND_NONE => None,
            n => Some(n),
        },
    })
}

/// Require a present, non-absent string field; absent FlatBuffers strings on a
/// mandatory slot are a decode error.
fn str_field(value: Option<&str>, ctx: &str) -> Result<String, String> {
    value
        .map(str::to_string)
        .ok_or_else(|| format!("{ctx}: missing required string field"))
}

/// Collect an optional `[uint32]` vector into a `Vec<u32>`; absent == empty.
fn u32_vec(value: Option<flatbuffers::Vector<'_, u32>>) -> Vec<u32> {
    match value {
        Some(v) => v.iter().collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
