//! Segment IR — the cross-platform output of [`crate::tokenize`].
//!
//! These types are the internal, ergonomic, **recursive** representation. They
//! are NOT themselves serde-serializable (see the [`Segment`] doc-comment) and
//! therefore are NOT directly an FFI payload. The **FFI-stable wire boundary**
//! every consuming UI (SwiftUI / Compose / iced / wasm) actually decodes is
//! [`crate::wire::ContentTreeWire`], produced by the pure projection
//! [`ContentTree::to_wire`]. See
//! `docs/decisions/0018-content-tree-ffi-projection.md`. Changing a variant's
//! shape still ripples through the wire projection, so add fields
//! conservatively.

use nmp_core::nip21::NostrUri;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::markdown::MarkdownNode;
use crate::mode::RenderMode;

// MediaKind + InvoiceKind still derive serde because they're useful for
// diagnostics + iOS bridge JSON; they don't transitively contain NostrUri.

/// One token emitted by the tokenizer. See `content-rendering.md` §5.
///
/// Renderers walk a [`ContentTree`]'s `segments` and dispatch per variant.
/// The variants are deliberately **not** open-ended — a new segment kind is
/// a load-bearing decision that affects every platform's renderer.
///
/// `Segment` deliberately does NOT derive `Serialize`/`Deserialize` —
/// `NostrUri` (from `nmp-core`) lacks serde and we don't want this crate
/// to force its derivation. FFI consumers project to platform-native types
/// at the bridge; cross-process serialization is out of scope for Layer A.
#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    /// Inline text run — passes through unchanged (already escaped/decoded
    /// per the input).
    Text(String),
    /// Profile mention (NIP-21 `nostr:npub…` / `nostr:nprofile…`).
    Mention(NostrUri),
    /// Event reference (NIP-21 `nostr:note…` / `nostr:nevent…` /
    /// `nostr:naddr…`).
    EventRef(NostrUri),
    /// `#hashtag` token (without the leading `#`, lowercased).
    Hashtag(String),
    /// Plain URL (not classified as media).
    Url(Url),
    /// One or more media URLs grouped into a single segment by the
    /// post-pass grouper.
    Media {
        /// Ordered URLs comprising this media block.
        urls: Vec<Url>,
        /// Best-guess classification from URL extension.
        kind: MediaKind,
    },
    /// NIP-30 custom emoji shortcode (`:foo:`) optionally paired with the
    /// `emoji` tag URL if found in the event's tags.
    Emoji {
        /// The shortcode between `:` markers (without colons).
        shortcode: String,
        /// Image URL resolved from the event's `emoji` tags, or `None`
        /// when unresolved.
        url: Option<Url>,
    },
    /// Reserved invoice segment. The substrate emits these so apps can
    /// render pay UX; the actual wallet integration is app-owned (M12
    /// deferred). Variants cover the common Nostr-adjacent payment formats.
    Invoice(InvoiceKind),
    /// Populated only in `RenderMode::Markdown` — a block-level markdown
    /// node whose inline runs reuse the same inline `Segment` shape.
    MarkdownBlock(MarkdownNode),
}

/// Classifier emitted alongside `Segment::Media`. Pure URL-extension
/// inference — no MIME sniff, no HTTP fetch.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MediaKind {
    /// `.jpg`, `.jpeg`, `.png`, `.gif`, `.webp`, `.svg`, `.avif`, `.heic`.
    Image,
    /// `.mp4`, `.mov`, `.webm`, `.m4v`, `.mkv`.
    Video,
    /// `.mp3`, `.m4a`, `.wav`, `.ogg`, `.flac`, `.opus`.
    Audio,
}

/// Reserved invoice segment kinds. Substrate detects + emits; renderers
/// (wallet UX) are app-owned.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum InvoiceKind {
    /// `lnbc…` Lightning Network invoice (BOLT-11).
    Bolt11(String),
    /// `lno…` Lightning offer (BOLT-12).
    Bolt12(String),
    /// `cashuA…` Cashu token (NUT-00).
    Cashu(String),
}

/// Result of [`crate::tokenize`]. Stable across re-tokenization passes for
/// the same `(content, tags, mode)` triple.
///
/// Does not derive serde for the same reason as [`Segment`] — see that
/// type's doc-comment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContentTree {
    /// Flat sequence of inline + (in Markdown mode) block segments.
    pub segments: Vec<Segment>,
    /// The mode the tree was produced under. `RenderMode::Auto` always
    /// resolves to either `Plain` or `Markdown` before being stored here.
    pub mode: RenderMode,
}

impl ContentTree {
    /// Construct an empty tree (used when content is empty or every token
    /// was a parse-error fallback).
    pub fn empty(mode: RenderMode) -> Self {
        Self {
            segments: Vec::new(),
            mode,
        }
    }

    /// Convenience: count tokens of each variant. Used by tests/diagnostics
    /// rather than rendering — keep this stable.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tree_has_zero_segments() {
        let tree = ContentTree::empty(RenderMode::Plain);
        assert_eq!(tree.segment_count(), 0);
        assert_eq!(tree.mode, RenderMode::Plain);
    }

    #[test]
    fn text_segment_equality_holds() {
        let s = Segment::Text("hello world".to_string());
        assert_eq!(s, Segment::Text("hello world".to_string()));
        assert_ne!(s, Segment::Text("other".to_string()));
    }

    #[test]
    fn hashtag_segment_equality_holds() {
        let s = Segment::Hashtag("nostr".to_string());
        assert_eq!(s, Segment::Hashtag("nostr".to_string()));
    }

    #[test]
    fn invoice_kind_variants_round_trip_through_json() {
        let bolt11 = InvoiceKind::Bolt11("lnbc1qq".to_string());
        let bolt12 = InvoiceKind::Bolt12("lno1qq".to_string());
        let cashu = InvoiceKind::Cashu("cashuAeyJ".to_string());
        for kind in [bolt11, bolt12, cashu] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: InvoiceKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn media_kind_variants_round_trip_through_json() {
        for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: MediaKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }
}
