//! Serde-derivable mirror of the `nmp_content` IR.
//!
//! `nmp_content::Segment` / `ContentTree` / `MarkdownNode` deliberately do
//! NOT derive serde (see `crates/nmp-content/src/segment.rs`), and there is
//! no live `ContentTree` FFI projection (T93 "ContentTree FFI ADR" is in
//! flight, not landed). This module is the candidate projection shape
//! offered as input to T93: a 1:1 tagged-union mirror that crosses the
//! Rust↔Swift boundary as JSON. We import the `nmp_content` types as-is and
//! project them here (`super::project`) — consumption, not editing.

use nmp_content::EmbedKindProjection;
use serde::{Deserialize, Serialize};

/// Serde mirror of `nmp_content::ContentTree`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ContentTreeDto {
    /// `"Plain"` or `"Markdown"` — resolved render mode.
    pub mode: String,
    /// Flat sequence of inline + (Markdown mode) block segments.
    pub segments: Vec<SegmentDto>,
}

/// Serde mirror of `nmp_content::Segment`. `type` is the discriminator.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SegmentDto {
    /// Inline text run.
    Text { text: String },
    /// Profile mention (`nostr:npub…` / `nostr:nprofile…`).
    Mention {
        /// Canonical `nostr:` URI.
        uri: String,
        /// `"npub"` (no relays) or `"nprofile"` (relay hints present).
        kind: String,
        /// 32-byte pubkey hex.
        pubkey: String,
    },
    /// Event reference (`nostr:note…` / `nostr:nevent…` / `nostr:naddr…`).
    EventRef {
        /// Canonical `nostr:` URI.
        uri: String,
        /// `"note"`, `"nevent"`, or `"naddr"`.
        kind: String,
        /// Event id hex (note/nevent) or `kind:pubkey:d` coord (naddr).
        id: String,
    },
    /// `#hashtag` (without leading `#`, lowercased).
    Hashtag { tag: String },
    /// Plain URL (not classified media).
    Url { url: String },
    /// Plain `http(s)` URL that MIGHT double as a NIP-AD pointer (#2927). Renders
    /// as a plain link baseline; the distinct kind marks it claimable.
    AdCandidateUrl { url: String },
    /// One or more grouped media URLs.
    Media {
        /// `"Image"`, `"Video"`, or `"Audio"`.
        media_kind: String,
        /// Ordered URLs comprising the media block.
        urls: Vec<String>,
    },
    /// NIP-30 custom emoji.
    Emoji {
        /// Shortcode between `:` markers.
        shortcode: String,
        /// Resolved emoji-tag URL, or `None` when unresolved.
        url: Option<String>,
    },
    /// Reserved invoice token (wallet UX app-owned, M12 deferred).
    Invoice {
        /// `"Bolt11"`, `"Bolt12"`, or `"Cashu"`.
        invoice_kind: String,
        /// Raw token value.
        value: String,
    },
    /// Markdown block (Markdown mode only).
    MarkdownBlock { node: MarkdownNodeDto },
}

/// Serde mirror of `nmp_content::MarkdownNode` (CommonMark-core, PD-012).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MarkdownNodeDto {
    /// `# heading` — level 1-6.
    Heading {
        /// `#` count.
        level: u8,
        /// Inline runs.
        inlines: Vec<MarkdownInlineDto>,
    },
    /// Paragraph of inline runs.
    Paragraph { inlines: Vec<MarkdownInlineDto> },
    /// Block quote (nested blocks).
    BlockQuote { blocks: Vec<MarkdownNodeDto> },
    /// Fenced/indented code block.
    CodeBlock {
        /// Optional language token.
        info: Option<String>,
        /// Raw code body.
        body: String,
    },
    /// Bullet or ordered list.
    List {
        /// Ordered start, or `None` for bullet.
        ordered_start: Option<u64>,
        /// Each item is a list of nested blocks.
        items: Vec<Vec<MarkdownNodeDto>>,
    },
    /// Horizontal rule.
    Rule,
}

/// Serde mirror of `nmp_content::MarkdownInline`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MarkdownInlineDto {
    /// One inline `Segment` variant.
    Inline { segment: SegmentDto },
    /// `*italic*`.
    Emphasis { children: Vec<MarkdownInlineDto> },
    /// `**bold**`.
    Strong { children: Vec<MarkdownInlineDto> },
    /// `` `code` ``.
    Code { text: String },
    /// `[label](href)`.
    Link {
        /// Display label (inline runs).
        label: Vec<MarkdownInlineDto>,
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
    /// Soft line break (single `\n`).
    SoftBreak,
    /// Hard line break.
    HardBreak,
}

/// Standard Nostr event object — real signature + valid id.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SignedEventJson {
    /// Event id (lowercase hex).
    pub id: String,
    /// Author pubkey (lowercase hex).
    pub pubkey: String,
    /// Unix-second creation time.
    pub created_at: u64,
    /// Event kind.
    pub kind: u32,
    /// Tag rows.
    pub tags: Vec<Vec<String>>,
    /// Content payload.
    pub content: String,
    /// Schnorr signature (lowercase hex).
    pub sig: String,
}

/// A pre-resolved embed entry in the relay-free fixture store.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EmbedEntry {
    /// Opaque renderer cycle key produced by Rust. Native renderers pass this
    /// through their traversal guard; they must not re-derive it from kind/tags.
    pub cycle_key: String,
    /// Resolved event kind (0 = profile metadata).
    pub resolved_kind: u32,
    /// kind:0 display name, when this entry is a profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_name: Option<String>,
    /// kind:0 picture URL, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<String>,
    /// The resolved underlying event, when this entry is an event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<SignedEventJson>,
    /// Rendered content of the resolved event (recursion-guarded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered: Option<ContentTreeDto>,
    /// Whether the renderer collapsed this embed.
    pub collapsed: bool,
    /// `null` | `"depth"` | `"cycle"` | `"unsupported"` | `"dangling"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapse_reason: Option<String>,
    /// Typed per-kind projection from the canonical kernel resolver
    /// (`nmp_content::resolve_embed_projection`). This is the single shape
    /// native registries dispatch on (`article` → article card, `highlight`
    /// → highlight card, etc.); the legacy hand-rolled `article`/`list`
    /// fields are gone (#1998). `None` for dangling / non-event entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind_projection: Option<EmbedKindProjection>,
}

/// One showcase scenario in the bundle.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScenarioDto {
    /// Stable id (e.g. `S-T01`).
    pub id: String,
    /// Renderer category: text/mentions/quotes/articles/lists/fallback.
    pub category: String,
    /// Human title.
    pub title: String,
    /// What nmp-content path + doctrine this exercises.
    pub exercises: String,
    /// Signed event(s) — real sigs, valid ids.
    pub events: Vec<SignedEventJson>,
    /// Rendered content of the primary event (real `tokenize_with_kind`).
    pub rendered: ContentTreeDto,
    /// Relay-free pre-resolved embed store, keyed by `nostr:` URI.
    pub embeds: std::collections::BTreeMap<String, EmbedEntry>,
}

/// Top-level bundle envelope shipped as an iOS app resource.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bundle {
    /// Schema version.
    pub version: u32,
    /// Every showcase scenario.
    pub scenarios: Vec<ScenarioDto>,
}
