//! Typed per-kind projections for embedded events.
//!
//! These are the data shapes emitted by the Rust resolver for one embedded
//! Nostr event. The variant drives native widget dispatch; the payload is the
//! complete typed data the widget renders — it never re-parses the raw event.
//!
//! All fields follow ADR-0072 (raw protocol data only): pubkeys as 64-char
//! lowercase hex, timestamps as Unix u64 seconds, no pre-computed strings.
//!
//! Display separation (#2514): every non-`Profile` projection carries ONLY the
//! raw `author_pubkey` hex. Author display (`display_name`/`picture`) is the
//! exclusive province of [`ProfileProjection`] (the kind:0 primitive) and joins
//! reactively at the L5 composition layer keyed by `author_pubkey`.

use serde::{Deserialize, Serialize};

use crate::wire::ContentTreeWire;

/// Typed data envelope emitted by the Rust resolver for one embedded event.
/// The variant tag drives native widget dispatch; the variant payload is the
/// complete typed data the widget renders — it never re-parses the raw event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "variant", content = "data", rename_all = "camelCase")]
pub enum EmbedKindProjection {
    /// Kind:1 short text note projection.
    ShortNote(ShortNoteProjection),
    /// Kind:30023 long-form article projection.
    Article(ArticleProjection),
    /// Kind:9802 highlight projection.
    Highlight(HighlightProjection),
    /// Kind:0 profile metadata projection.
    Profile(ProfileProjection),
    /// Fallback projection for all unregistered or unsupported kinds.
    Unknown(UnknownProjection),
}

impl EmbedKindProjection {
    /// The single raw pubkey this projection's display name/avatar reactively
    /// joins against at L5 (#3016) — the `author_pubkey` for every kind that
    /// renders one, or `Profile`'s own `pubkey` (a `Profile` embed's "author"
    /// is the profile subject itself). `None` for an empty/absent key so a
    /// caller never demands a resolve for `""`.
    ///
    /// This is the seam a host-side embed sidecar uses to auto-`resolve_ref`
    /// every author an embed will render — the SAME "component-owned kind:0
    /// claiming" pattern a feed row's author already gets via
    /// `register_feed_author_provider`/`FeedAuthorRefs`. Without it, an
    /// embedded event's author never gets a live resolver demand unless some
    /// OTHER surface happens to already be watching that pubkey, so its
    /// display name never arrives no matter how long a host waits (#3016 bug
    /// 2: a quoted note's author kind:0 never resolves).
    #[must_use]
    pub fn referenced_pubkey(&self) -> Option<&str> {
        let key = match self {
            EmbedKindProjection::ShortNote(p) => p.author_pubkey.as_str(),
            EmbedKindProjection::Article(p) => p.author_pubkey.as_str(),
            EmbedKindProjection::Highlight(p) => p.author_pubkey.as_str(),
            EmbedKindProjection::Profile(p) => p.pubkey.as_str(),
            EmbedKindProjection::Unknown(p) => p.author_pubkey.as_str(),
        };
        (!key.is_empty()).then_some(key)
    }
}

/// Projection payload for a kind:1 short text note embed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortNoteProjection {
    /// 64-character hex event id.
    pub id: String,
    /// 64-character hex author pubkey (display joins reactively at L5).
    pub author_pubkey: String,
    /// Event creation time as Unix seconds.
    pub created_at: u64,
    /// Full rendered content tree for the note body.
    pub content_tree: ContentTreeWire,
    /// Top-level media URLs extracted for preview thumbnails.
    pub media_urls: Vec<String>,
}

/// Projection payload for a kind:30023 long-form article embed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleProjection {
    /// 64-character hex event id.
    pub id: String,
    /// 64-character hex author pubkey (display joins reactively at L5).
    pub author_pubkey: String,
    /// Event creation time as Unix seconds.
    pub created_at: u64,
    /// Optional `title` tag value.
    pub title: Option<String>,
    /// Optional `summary` tag value.
    pub summary: Option<String>,
    /// Optional `image` tag value used as a hero image URL.
    pub hero_image_url: Option<String>,
    /// Addressable `d` tag value.
    pub d_tag: String,
    /// Full rendered content tree for the article body.
    pub content_tree: ContentTreeWire,
}

/// Projection payload for a kind:9802 highlight embed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightProjection {
    /// 64-character hex event id.
    pub id: String,
    /// 64-character hex author pubkey (display joins reactively at L5).
    pub author_pubkey: String,
    /// Event creation time as Unix seconds.
    pub created_at: u64,
    /// Highlighted text from the event content.
    pub highlighted_text: String,
    /// Optional `e` tag when the highlight points at a note.
    pub source_event_id: Option<String>,
    /// Optional `a` tag when the highlight points at an addressable event.
    pub source_event_addr: Option<String>,
    /// Optional `r` tag when the highlight points at a web URL.
    pub source_url: Option<String>,
    /// Optional `context` tag containing surrounding text.
    pub context: Option<String>,
}

/// Projection payload for a kind:0 profile embed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileProjection {
    /// 64-character hex profile pubkey.
    pub pubkey: String,
    /// Optional display name copied verbatim from the profile event.
    pub display_name: Option<String>,
    /// Optional picture URL copied verbatim from the profile event.
    pub picture_url: Option<String>,
    /// Optional profile about text copied verbatim from the profile event.
    pub about: Option<String>,
    /// Optional NIP-05 identifier copied verbatim from the profile event.
    pub nip05: Option<String>,
    /// Optional Lightning address copied verbatim from the profile event.
    pub lud16: Option<String>,
    /// Optional banner URL copied verbatim from the profile event.
    pub banner_url: Option<String>,
}

/// Projection payload for an embed kind without a registered Rust projection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnknownProjection {
    /// Raw Nostr event kind.
    pub kind: u32,
    /// 64-character hex author pubkey (display joins reactively at L5).
    pub author_pubkey: String,
    /// Event creation time as Unix seconds.
    pub created_at: u64,
    /// Raw event content.
    pub content: String,
    /// Parsed content tree using the same renderer path as known text kinds.
    pub content_tree: ContentTreeWire,
    /// Raw NIP-01 tags available to custom native renderers.
    pub tags: Vec<Vec<String>>,
    /// Optional NIP-31 `alt` tag value.
    pub alt_text: Option<String>,
}
