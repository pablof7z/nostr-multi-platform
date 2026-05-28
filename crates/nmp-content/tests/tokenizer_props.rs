//! Property-based invariants for the tokenizer.
//!
//! Per the brief: at least **4 proptest invariants**. Each invariant is a
//! cross-input claim that any random content respects:
//!
//! 1. **Text round-trips.** Pure-text content with no tokenizable
//!    substrings emits exactly one `Segment::Text` carrying the original
//!    string verbatim.
//! 2. **Invalid bech32 falls back to text.** Random `nostr:` prefixes
//!    followed by garbage never panic and never produce a `Mention` /
//!    `EventRef` — they decay to a `Text` segment.
//! 3. **Hashtag tokens are lowercased and non-empty.** Every emitted
//!    `Hashtag` segment carries a lowercase, non-empty body.
//! 4. **Recursion guard short-circuits at depth 4.** A `RenderContext`
//!    descended `N` times where `N >= max_depth` MUST report
//!    `should_collapse == true` for any candidate child id.
//! 5. **Source round-trips (no character drops).** For arbitrary
//!    lowercase-ASCII content (which the tokenizer never normalizes),
//!    concatenating each segment's source reproduces the input verbatim —
//!    every source character is preserved (codex finding #7/#8: leading
//!    whitespace before hashtags and trimmed URL punctuation were dropped).

use nmp_content::{render_context_can_descend, tokenize, RenderContext, RenderMode, Segment};
use proptest::prelude::*;

/// Reconstruct the source string a `Segment` was tokenized from. Only the
/// variants the round-trip generators can produce are handled — `Url`,
/// `Media`, markdown blocks, etc. are unreachable for lowercase-ASCII
/// no-URL input and panic loudly if that assumption ever breaks.
fn segment_source(seg: &Segment) -> String {
    match seg {
        Segment::Text(t) => t.clone(),
        // Hashtag is lowercased + strips the `#`; lowercase-ASCII input is
        // already lowercase so reconstruction is exact.
        Segment::Hashtag(tag) => format!("#{tag}"),
        Segment::Emoji { shortcode, .. } => format!(":{shortcode}:"),
        other => panic!("unexpected segment for round-trip input: {other:?}"),
    }
}

fn reconstruct(segments: &[Segment]) -> String {
    segments.iter().map(segment_source).collect()
}

/// Hashtag-class punctuation that breaks token boundaries — keep these out
/// of the "plain text" generator so the tokenizer doesn't pull a tag out.
fn arbitrary_safe_text() -> impl Strategy<Value = String> {
    // ASCII letters + spaces — no `#`, `:`, `/`, `\`, `<`, `>`, `"`, `'`,
    // `\` or non-ASCII (which would otherwise trigger `\p{L}` matches).
    proptest::collection::vec(
        proptest::char::range('a', 'z').prop_filter("ascii letter or space", |c| {
            c.is_ascii_lowercase() || *c == ' '
        }),
        0..120,
    )
    .prop_map(|chars| chars.into_iter().collect::<String>())
}

proptest! {
    #[test]
    fn plain_text_round_trips_when_no_tokens(content in arbitrary_safe_text()) {
        let tree = tokenize(&content, &[], RenderMode::Plain);
        if content.is_empty() {
            prop_assert!(tree.segments.is_empty());
        } else {
            prop_assert_eq!(tree.segments.len(), 1);
            prop_assert_eq!(&tree.segments[0], &Segment::Text(content));
        }
    }

    #[test]
    fn invalid_bech32_falls_back_to_text(suffix in "[a-z0-9]{1,32}") {
        // Random `nostr:npub1<garbage>` — pattern matches, NIP-19 parse
        // fails, the classifier MUST emit `Text` instead of panicking.
        let content = format!("nostr:npub1{suffix}");
        let tree = tokenize(&content, &[], RenderMode::Plain);
        for seg in &tree.segments {
            match seg {
                Segment::Mention(_) | Segment::EventRef(_) => {
                    prop_assert!(false, "invalid bech32 should not parse to entity: {seg:?}");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn hashtag_tokens_lowercased_and_non_empty(name in "[A-Za-z][A-Za-z0-9_]{0,20}") {
        let content = format!("see #{name} here");
        let tree = tokenize(&content, &[], RenderMode::Plain);
        let tags: Vec<_> = tree
            .segments
            .iter()
            .filter_map(|s| if let Segment::Hashtag(t) = s { Some(t.clone()) } else { None })
            .collect();
        prop_assert_eq!(tags.len(), 1);
        prop_assert!(!tags[0].is_empty());
        prop_assert_eq!(tags[0].clone(), tags[0].to_lowercase());
    }

    #[test]
    fn recursion_guard_short_circuits_at_or_past_max_depth(
        descents in 0u8..16,
        candidate in "[a-f0-9]{16}",
    ) {
        let mut ctx = RenderContext::new();
        for i in 0..descents {
            // Descend with a unique id so visited-set doesn't trigger.
            ctx = ctx.descend(format!("desc-{i}"));
        }
        let collapse = ctx.should_collapse(&candidate);
        let can = render_context_can_descend(&ctx, &candidate);
        prop_assert_eq!(collapse, !can);
        if descents >= ctx.max_depth {
            prop_assert!(collapse, "depth {descents} >= max {} should collapse", ctx.max_depth);
        }
    }

    /// Invariant 5: arbitrary lowercase-ASCII content (letters, digits,
    /// spaces, newlines, `#`, ASCII punctuation) round-trips with zero
    /// character loss. The tokenizer never normalizes lowercase ASCII, so
    /// `reconstruct(segments) == content` must hold exactly.
    #[test]
    fn source_round_trips_without_dropping_characters(
        content in r"[a-z0-9 \n#.,;:!?()_-]{0,160}"
    ) {
        let tree = tokenize(&content, &[], RenderMode::Plain);
        prop_assert_eq!(reconstruct(&tree.segments), content);
    }
}

/// Regression: codex finding #7 — leading whitespace before a hashtag was
/// dropped from the source text on round-trip.
#[test]
fn hashtag_preserves_leading_whitespace() {
    let tree = tokenize("hello   #nostr there", &[], RenderMode::Plain);
    assert_eq!(reconstruct(&tree.segments), "hello   #nostr there");
}

/// Regression: codex finding #7 with newline + tab separators.
#[test]
fn hashtag_preserves_newline_separator() {
    let tree = tokenize("line\n#tag", &[], RenderMode::Plain);
    assert_eq!(reconstruct(&tree.segments), "line\n#tag");
}

/// Regression: codex finding #8 — URL trailing punctuation was trimmed off
/// the URL then silently lost (never re-emitted as text).
#[test]
fn url_trailing_punctuation_re_emitted_as_text() {
    let tree = tokenize("see https://example.com/x).", &[], RenderMode::Plain);
    let urls: Vec<_> = tree
        .segments
        .iter()
        .filter(|s| matches!(s, Segment::Url(_) | Segment::Media { .. }))
        .collect();
    assert_eq!(urls.len(), 1, "exactly one URL segment");
    // The trimmed `).` suffix must survive as a trailing Text segment.
    let trailing_text: String = tree
        .segments
        .iter()
        .filter_map(|s| {
            if let Segment::Text(t) = s {
                Some(t.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        trailing_text.contains(")."),
        "trimmed URL punctuation `).` must be re-emitted as text, got segments: {:?}",
        tree.segments
    );
}
