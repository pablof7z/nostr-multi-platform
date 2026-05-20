//! Single public tokenizer entry point — [`tokenize`].
//!
//! Per `content-rendering.md` §10 anti-pattern #1, there is **one** way in.
//! The `mode` parameter selects `Plain` vs `Markdown` (Auto resolves via
//! [`crate::mode::sniff_mode_from_kind`] when a kind hint is provided).

use std::collections::HashMap;

use nmp_core::nip21::parse_nostr_uri;
use url::Url;

use crate::grouper::group_consecutive_media;
use crate::markdown::parse_markdown_blocks;
use crate::mode::{sniff_mode_from_kind, RenderMode};
use crate::regex_set::{patterns, PatternKind};
use crate::segment::{ContentTree, InvoiceKind, Segment};

/// Tokenize `content` into a [`ContentTree`].
///
/// `tags` are the parent event's tag rows (`[["e", "<id>"], ["emoji", "foo",
/// "https://…/foo.png"], …]`) — used to resolve NIP-30 custom emoji
/// shortcodes and (in the future) `#[i]` legacy tag references. Pass an
/// empty slice if no tags are available.
///
/// `mode` selects the parser. `RenderMode::Auto` requires a separate
/// `kind` hint via [`tokenize_with_kind`]; when called as `tokenize(_, _,
/// Auto)` without a kind, it degrades to `Plain`.
pub fn tokenize(content: &str, tags: &[Vec<String>], mode: RenderMode) -> ContentTree {
    let resolved = match mode {
        RenderMode::Auto => RenderMode::Plain,
        other => other,
    };
    tokenize_resolved(content, tags, resolved)
}

/// Tokenize with an explicit kind hint — used when `mode` is `Auto`. If
/// `mode` is already `Plain` / `Markdown`, the kind is ignored.
pub fn tokenize_with_kind(
    content: &str,
    tags: &[Vec<String>],
    mode: RenderMode,
    kind: u32,
) -> ContentTree {
    let resolved = match mode {
        RenderMode::Auto => sniff_mode_from_kind(kind),
        other => other,
    };
    tokenize_resolved(content, tags, resolved)
}

fn tokenize_resolved(content: &str, tags: &[Vec<String>], mode: RenderMode) -> ContentTree {
    if content.is_empty() {
        return ContentTree::empty(mode);
    }
    let emoji_table = build_emoji_table(tags);
    let segments = match mode {
        RenderMode::Markdown => parse_markdown_blocks(content, &emoji_table),
        RenderMode::Plain | RenderMode::Auto => {
            tokenize_inline_with_grouping(content, &emoji_table)
        }
    };
    ContentTree { segments, mode }
}

/// Tokenize an inline run (no markdown blocks). Public to `markdown.rs`
/// so block inlines reuse the same tokenizer — the "one parser, two render
/// paths" invariant from §10 #3.
pub(crate) fn tokenize_inline(content: &str, emoji_table: &HashMap<String, Url>) -> Vec<Segment> {
    if content.is_empty() {
        return Vec::new();
    }
    let matches = collect_matches(content);
    let resolved = resolve_overlaps(matches);
    classify(content, resolved, emoji_table)
}

fn tokenize_inline_with_grouping(
    content: &str,
    emoji_table: &HashMap<String, Url>,
) -> Vec<Segment> {
    let inline = tokenize_inline(content, emoji_table);
    group_consecutive_media(inline)
}

/// One regex match — kept as raw byte offsets so we can slice `content`
/// directly.
#[derive(Clone, Debug)]
struct RawMatch {
    start: usize,
    end: usize,
    capture_start: usize,
    capture_end: usize,
    kind: PatternKind,
}

fn collect_matches(content: &str) -> Vec<RawMatch> {
    let mut out = Vec::new();
    for pat in patterns() {
        for caps in pat.re.captures_iter(content) {
            let whole = caps.get(0).expect("group 0 always exists"); // doctrine-allow: D6 — `regex` crate API guarantees capture group 0 (the whole match) is always present for any `Captures`; this is a type-level invariant, not an operational failure
            // Hashtag uses group 1 to skip the leading whitespace; everything
            // else uses group 0.
            let cap = caps.get(1).unwrap_or(whole);
            out.push(RawMatch {
                start: whole.start(),
                end: whole.end(),
                capture_start: cap.start(),
                capture_end: cap.end(),
                kind: pat.kind,
            });
        }
    }
    out.sort_by_key(|m| (m.start, std::cmp::Reverse(m.end - m.start)));
    out
}

/// Resolve overlapping matches by preferring the earlier-starting,
/// longer-spanning one. Ties go to the first match in the sorted order.
fn resolve_overlaps(matches: Vec<RawMatch>) -> Vec<RawMatch> {
    let mut out: Vec<RawMatch> = Vec::with_capacity(matches.len());
    for m in matches {
        if let Some(last) = out.last() {
            if m.start < last.end {
                continue;
            }
        }
        out.push(m);
    }
    out
}

fn classify(
    content: &str,
    matches: Vec<RawMatch>,
    emoji_table: &HashMap<String, Url>,
) -> Vec<Segment> {
    let mut out = Vec::with_capacity(matches.len() * 2 + 1);
    let mut cursor = 0usize;
    for m in matches {
        if m.start > cursor {
            let between = &content[cursor..m.start];
            if !between.is_empty() {
                out.push(Segment::Text(between.to_string()));
            }
        }
        let raw = &content[m.start..m.end];
        let capture = &content[m.capture_start..m.capture_end];
        match m.kind {
            // Hashtag's match span includes the leading `(?:^|\s)`
            // separator before the captured `#tag`. Re-emit that separator
            // as Text so the source round-trips (codex finding #7). The `#`
            // sits at `capture_start - 1`; the separator is everything
            // before it (empty when matched at start-of-string).
            PatternKind::Hashtag => {
                let prefix = &content[m.start..m.capture_start - 1];
                if !prefix.is_empty() {
                    out.push(Segment::Text(prefix.to_string()));
                }
                if let Some(seg) = classify_match(m.kind, raw, capture, emoji_table) {
                    out.push(seg);
                } else {
                    // Empty tag body — re-emit the whole match verbatim.
                    out.push(Segment::Text(raw.to_string()));
                }
            }
            // URLs trim trailing punctuation; that suffix must survive as a
            // trailing Text segment, never silently dropped (finding #8).
            PatternKind::Url => match parse_url_segment(raw) {
                Some((seg, trailing)) => {
                    out.push(seg);
                    if !trailing.is_empty() {
                        out.push(Segment::Text(trailing.to_string()));
                    }
                }
                None => out.push(Segment::Text(raw.to_string())),
            },
            _ => {
                if let Some(seg) = classify_match(m.kind, raw, capture, emoji_table) {
                    out.push(seg);
                } else {
                    out.push(Segment::Text(raw.to_string()));
                }
            }
        }
        cursor = m.end;
    }
    if cursor < content.len() {
        let tail = &content[cursor..];
        if !tail.is_empty() {
            out.push(Segment::Text(tail.to_string()));
        }
    }
    coalesce_text(out)
}

fn classify_match(
    kind: PatternKind,
    raw: &str,
    capture: &str,
    emoji_table: &HashMap<String, Url>,
) -> Option<Segment> {
    match kind {
        PatternKind::NostrUri => match parse_nostr_uri(raw) {
            Ok(uri) => {
                use nmp_core::nip21::NostrUri;
                match uri {
                    NostrUri::Profile { .. } => Some(Segment::Mention(uri)),
                    NostrUri::Event { .. } | NostrUri::Address { .. } => Some(Segment::EventRef(uri)),
                }
            }
            Err(_) => None,
        },
        PatternKind::EmojiShortcode => Some(Segment::Emoji {
            shortcode: capture.to_string(),
            url: emoji_table.get(capture).cloned(),
        }),
        PatternKind::Hashtag => {
            let tag = capture.trim_start_matches('#').to_lowercase();
            if tag.is_empty() {
                None
            } else {
                Some(Segment::Hashtag(tag))
            }
        }
        // URL is handled directly in `classify` so its trimmed trailing
        // punctuation can be re-emitted as a sibling Text segment.
        PatternKind::Url => None,
        PatternKind::Bolt11 => Some(Segment::Invoice(InvoiceKind::Bolt11(raw.to_string()))),
        PatternKind::Bolt12 => Some(Segment::Invoice(InvoiceKind::Bolt12(raw.to_string()))),
        PatternKind::Cashu => Some(Segment::Invoice(InvoiceKind::Cashu(raw.to_string()))),
    }
}

/// Strip trailing punctuation, then parse. On success returns the
/// `Segment::Url` plus the trimmed trailing-punctuation suffix (a slice of
/// `raw`) so the caller can re-emit it as Text — no source character is
/// ever dropped (codex finding #8). Returns `None` (text fallback) for
/// unparseable URLs.
fn parse_url_segment(raw: &str) -> Option<(Segment, &str)> {
    let trimmed = raw.trim_end_matches(['.', ',', ';', ':', '!', '?', ')']);
    let url = Url::parse(trimmed).ok()?;
    // The trim chars are all ASCII, so the byte-length delta is a valid
    // char boundary on `raw`.
    let trailing = &raw[trimmed.len()..];
    Some((Segment::Url(url), trailing))
}

fn coalesce_text(input: Vec<Segment>) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::with_capacity(input.len());
    for seg in input {
        match (out.last_mut(), seg) {
            (Some(Segment::Text(prev)), Segment::Text(next)) => {
                prev.push_str(&next);
            }
            (_, seg) => out.push(seg),
        }
    }
    out
}

/// Build a `shortcode → URL` table from the event's `emoji` tags. Per NIP-30:
/// `["emoji", "<shortcode>", "<url>"]`.
fn build_emoji_table(tags: &[Vec<String>]) -> HashMap<String, Url> {
    let mut table = HashMap::new();
    for row in tags {
        if row.len() >= 3 && row[0] == "emoji" {
            if let Ok(url) = Url::parse(&row[2]) {
                table.insert(row[1].clone(), url);
            }
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_content_returns_empty_tree() {
        let tree = tokenize("", &[], RenderMode::Plain);
        assert_eq!(tree.segments.len(), 0);
        assert_eq!(tree.mode, RenderMode::Plain);
    }

    #[test]
    fn plain_text_passes_through() {
        let tree = tokenize("hello world", &[], RenderMode::Plain);
        assert_eq!(tree.segments, vec![Segment::Text("hello world".to_string())]);
    }

    #[test]
    fn hashtag_emits_segment_lowercased() {
        let tree = tokenize("hello #Nostr there", &[], RenderMode::Plain);
        // The leading separator (" ") before `#` is preserved as Text so
        // the source round-trips (codex finding #7).
        assert_eq!(
            tree.segments,
            vec![
                Segment::Text("hello ".to_string()),
                Segment::Hashtag("nostr".to_string()),
                Segment::Text(" there".to_string()),
            ]
        );
    }

    #[test]
    fn url_emits_segment() {
        let tree = tokenize("see https://example.com/x", &[], RenderMode::Plain);
        assert_eq!(tree.segments.len(), 2);
        assert!(matches!(tree.segments[1], Segment::Url(_)));
    }

    #[test]
    fn image_url_promoted_by_grouper_to_media() {
        let tree = tokenize("https://x.test/a.jpg", &[], RenderMode::Plain);
        assert_eq!(tree.segments.len(), 1);
        assert!(matches!(
            tree.segments[0],
            Segment::Media { kind: crate::segment::MediaKind::Image, .. }
        ));
    }

    #[test]
    fn malformed_nostr_uri_falls_back_to_text() {
        let tree = tokenize("nostr:npub1notreallybech32", &[], RenderMode::Plain);
        // No URI-shaped match validates -> the regex still matches the prefix
        // but parse fails -> falls back to Text.
        assert!(matches!(tree.segments[0], Segment::Text(_)));
    }

    #[test]
    fn invoice_bolt11_emits_invoice_segment() {
        let body = format!("pay lnbc1{} please", "q".repeat(60));
        let tree = tokenize(&body, &[], RenderMode::Plain);
        assert!(tree
            .segments
            .iter()
            .any(|s| matches!(s, Segment::Invoice(InvoiceKind::Bolt11(_)))));
    }

    #[test]
    fn emoji_shortcode_resolves_from_tags() {
        let tags = vec![vec![
            "emoji".to_string(),
            "ostrich".to_string(),
            "https://x.test/ostrich.png".to_string(),
        ]];
        let tree = tokenize("hello :ostrich: there", &tags, RenderMode::Plain);
        let emoji_seg = tree
            .segments
            .iter()
            .find(|s| matches!(s, Segment::Emoji { .. }))
            .expect("emoji segment");
        if let Segment::Emoji { shortcode, url } = emoji_seg {
            assert_eq!(shortcode, "ostrich");
            assert!(url.is_some());
        }
    }

    #[test]
    fn auto_without_kind_degrades_to_plain() {
        let tree = tokenize("# heading", &[], RenderMode::Auto);
        assert_eq!(tree.mode, RenderMode::Plain);
        // Heading syntax is literal in plain mode.
        assert_eq!(tree.segments, vec![Segment::Text("# heading".to_string())]);
    }

    #[test]
    fn auto_with_kind_30023_picks_markdown() {
        let tree = tokenize_with_kind("# Heading", &[], RenderMode::Auto, 30023);
        assert_eq!(tree.mode, RenderMode::Markdown);
    }

    #[test]
    fn auto_with_kind_1_picks_plain() {
        let tree = tokenize_with_kind("hello #world", &[], RenderMode::Auto, 1);
        assert_eq!(tree.mode, RenderMode::Plain);
    }
}
