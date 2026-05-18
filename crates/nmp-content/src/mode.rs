//! [`RenderMode`] — the single mode flag the tokenizer accepts.
//!
//! Replaces NDKSwift's three overlapping APIs (`NDKRichText`, `NDKMarkdown`,
//! `NDKUIMarkdownRenderer`) with one entry point + this flag. See
//! `docs/research/content-rendering/ndkswift.md` §10 anti-pattern #1.

use serde::{Deserialize, Serialize};

/// Tokenizer mode — selects whether markdown block syntax is interpreted or
/// treated as literal text.
///
/// `Auto` delegates to [`sniff_mode_from_kind`] when an event kind is known;
/// callers that don't have a kind can pass [`RenderMode::Plain`] directly.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum RenderMode {
    /// Plain-text inline tokenization (mentions, URLs, hashtags, media,
    /// emoji, invoices). Markdown syntax such as `**bold**` passes through
    /// as literal text.
    Plain,
    /// Markdown block + inline tokenization. The block parser produces
    /// [`crate::markdown::MarkdownNode`] values; the inline pass within each
    /// block reuses the same [`crate::segment::Segment`] shape as `Plain`.
    Markdown,
    /// Resolve `Plain` vs `Markdown` from a kind hint at tokenize time.
    /// If no kind is supplied (`tokenize` called with `kind = None`), this
    /// degrades to `Plain` — matching the default for unknown content.
    #[default]
    Auto,
}

/// Map a Nostr event kind to the appropriate render mode.
///
/// Long-form articles (NIP-23, kind 30023/30024) and wiki articles
/// (NIP-54, kind 30818) render as Markdown. Everything else — short text
/// notes (kind 1), reposts (kind 6), reactions (kind 7), DMs, replaceable
/// kinds, contact lists, etc. — renders as plain text with inline tokens.
///
/// Update this table conservatively. Adding a kind here is a behavioral
/// change every consumer sees; prefer letting an app pass explicit
/// `RenderMode::Markdown` if the kind dispatch is app-specific.
pub fn sniff_mode_from_kind(kind: u32) -> RenderMode {
    match kind {
        // NIP-23 long-form content (markdown article).
        30023 | 30024 => RenderMode::Markdown,
        // NIP-54 wiki (markdown).
        30818 => RenderMode::Markdown,
        _ => RenderMode::Plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_is_default() {
        assert_eq!(RenderMode::default(), RenderMode::Auto);
    }

    #[test]
    fn sniffs_long_form_as_markdown() {
        assert_eq!(sniff_mode_from_kind(30023), RenderMode::Markdown);
        assert_eq!(sniff_mode_from_kind(30024), RenderMode::Markdown);
        assert_eq!(sniff_mode_from_kind(30818), RenderMode::Markdown);
    }

    #[test]
    fn sniffs_short_text_as_plain() {
        assert_eq!(sniff_mode_from_kind(1), RenderMode::Plain);
        assert_eq!(sniff_mode_from_kind(6), RenderMode::Plain);
        assert_eq!(sniff_mode_from_kind(7), RenderMode::Plain);
    }

    #[test]
    fn sniffs_unknown_kind_as_plain() {
        assert_eq!(sniff_mode_from_kind(0), RenderMode::Plain);
        assert_eq!(sniff_mode_from_kind(42), RenderMode::Plain);
        assert_eq!(sniff_mode_from_kind(9999), RenderMode::Plain);
        assert_eq!(sniff_mode_from_kind(u32::MAX), RenderMode::Plain);
    }
}
