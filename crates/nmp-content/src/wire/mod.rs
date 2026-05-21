//! `ContentTreeWire` — the serde-serializable FFI wire projection of
//! [`crate::ContentTree`]. See `docs/decisions/0018-content-tree-ffi-projection.md`.
//!
//! The internal [`crate::Segment`] / [`crate::MarkdownNode`] tree is recursive
//! and deliberately serde-free (it transitively contains
//! `nmp_core::nip21::NostrUri`, which has no serde derives). This module is the
//! **only** place serde derives live, and the only FFI-stable shape: a flat
//! index *arena* — `nodes: Vec<WireNode>` plus `roots: Vec<u32>` — with every
//! recursive parent→child edge expressed as explicit `u32` indices instead of
//! recursive borrows. That makes the JSON language-neutral, depth-bounded, and
//! `serde_derive`-able with zero custom impls.
//!
//! Projection is pure ([`ContentTree::to_wire`], in [`projection`]) and honours
//! D1 (best-effort: anything that cannot be projected becomes a typed
//! [`WireNode::Placeholder`], never a dropped subtree) and D6 (no panics — no
//! `unwrap`/`expect`/indexing that can panic on non-test paths).

mod projection;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

use crate::mode::RenderMode;
use crate::segment::{InvoiceKind, MediaKind};

/// Projection-internal nesting cap. **Not** the D1 render depth budget
/// (`RenderContext::max_depth`, default 4) — this only bounds the wire arena so
/// a pathologically deep / recursion-collapsed tree projects to a *finite*
/// form. At the cap a subtree collapses to a [`WireNode::Placeholder`] with
/// [`PlaceholderReason::DepthLimit`].
pub const WIRE_MAX_DEPTH: u32 = 32;

/// Flat, serde-serializable FFI projection of a [`crate::ContentTree`].
///
/// `nodes` is a single arena holding both block- and inline-level nodes.
/// `roots` is the top-level sequence (indices into `nodes`, in document order).
/// Every recursive child relationship in the internal tree is a `Vec<u32>` of
/// indices into `nodes` on the relevant [`WireNode`] variant.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ContentTreeWire {
    /// Flat arena of every node in the tree (block + inline kinds).
    pub nodes: Vec<WireNode>,
    /// Top-level node indices, in document order.
    pub roots: Vec<u32>,
    /// The mode the source tree was produced under.
    pub mode: RenderMode,
}

/// One node in the [`ContentTreeWire`] arena. Tagged enum — adding a variant is
/// the same load-bearing cross-platform decision adding a [`crate::Segment`]
/// variant already is (`content-rendering.md` §5).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireNode {
    /// Inline text run.
    Text {
        /// The literal text.
        text: String,
    },
    /// Profile mention (`Segment::Mention`).
    Mention {
        /// Flattened NIP-21 URI.
        uri: WireNostrUri,
    },
    /// Event / address reference (`Segment::EventRef`).
    EventRef {
        /// Flattened NIP-21 URI.
        uri: WireNostrUri,
    },
    /// `#hashtag` (without leading `#`, lowercased).
    Hashtag {
        /// The tag text.
        tag: String,
    },
    /// Plain URL.
    Url {
        /// The URL, serialized as its string form.
        url: String,
    },
    /// Grouped media block.
    Media {
        /// Ordered URLs as strings.
        urls: Vec<String>,
        /// URL-extension classification.
        media_kind: MediaKind,
    },
    /// NIP-30 custom emoji.
    Emoji {
        /// Shortcode between `:` markers.
        shortcode: String,
        /// Resolved image URL, or `None`.
        url: Option<String>,
    },
    /// Reserved payment segment.
    Invoice {
        /// The invoice payload.
        invoice: InvoiceKind,
    },
    /// Markdown heading.
    Heading {
        /// Level 1-6.
        level: u8,
        /// Inline child indices.
        children: Vec<u32>,
    },
    /// Markdown paragraph.
    Paragraph {
        /// Inline child indices.
        children: Vec<u32>,
    },
    /// Markdown block quote.
    BlockQuote {
        /// Block child indices.
        children: Vec<u32>,
    },
    /// Markdown fenced/indented code block (verbatim, never tokenized).
    CodeBlock {
        /// Optional language info string.
        info: Option<String>,
        /// Raw code body.
        body: String,
    },
    /// Markdown bullet/ordered list.
    List {
        /// `Some(n)` for an ordered list starting at `n`; `None` for bullet.
        ordered_start: Option<u64>,
        /// One entry per list item; each is that item's block child indices.
        items: Vec<Vec<u32>>,
    },
    /// Markdown horizontal rule.
    Rule,
    /// `*italic*` — children are inline node indices.
    Emphasis {
        /// Inline child indices.
        children: Vec<u32>,
    },
    /// `**bold**` — children are inline node indices.
    Strong {
        /// Inline child indices.
        children: Vec<u32>,
    },
    /// Inline `` `code` `` (verbatim).
    InlineCode {
        /// Raw code text.
        code: String,
    },
    /// `[label](href)` — `label` children are inline node indices.
    Link {
        /// Inline child indices for the label.
        children: Vec<u32>,
        /// Destination URL, or `None` if unparseable.
        href: Option<String>,
    },
    /// `![alt](src "title")`.
    Image {
        /// Alt text.
        alt: String,
        /// Optional title.
        title: Option<String>,
        /// Source URL, or `None` if unparseable.
        src: Option<String>,
    },
    /// Soft line break.
    SoftBreak,
    /// Hard line break.
    HardBreak,
    /// D1 placeholder: content existed here but could not be projected. Never
    /// a dropped subtree — always a typed, renderable node.
    Placeholder {
        /// Why this node replaced real content.
        reason: PlaceholderReason,
    },
}

/// Why a [`WireNode::Placeholder`] was emitted. Typed so renderers can decide
/// UX (a "thread too deep" affordance vs a broken-reference chip) without
/// string-matching.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceholderReason {
    /// Nesting exceeded [`WIRE_MAX_DEPTH`]; the subtree was collapsed.
    DepthLimit,
    /// A NIP-21 URI could not be formatted back to canonical form.
    UnresolvedUri,
}

/// Flattened, serde-serializable projection of `nmp_core::nip21::NostrUri`.
///
/// `uri` is the round-trippable canonical `nostr:` string; `kind` + `primary_id`
/// give the renderer the discriminator + pubkey/event-id hex without forcing it
/// to re-decode the bech32.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WireNostrUri {
    /// Canonical `nostr:` URI string (from `format_nostr_uri`).
    pub uri: String,
    /// Which NIP-21 entity this is.
    pub kind: WireNostrUriKind,
    /// Primary hex id: pubkey for `Profile`, event id for `Event`, author
    /// pubkey for `Address`.
    pub primary_id: String,
    /// Relay hints (may be empty).
    pub relays: Vec<String>,
    /// Author pubkey hex, for `Event` variants that carry one.
    pub author: Option<String>,
    /// Event kind, when the source entity carries one.
    pub event_kind: Option<u32>,
}

/// NIP-21 entity discriminator on the wire.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireNostrUriKind {
    /// `npub` / `nprofile`.
    Profile,
    /// `note` / `nevent`.
    Event,
    /// `naddr`.
    Address,
}
