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

use nmp_content::{
    render_context_can_descend, tokenize, RenderContext, RenderMode, Segment,
};
use proptest::prelude::*;

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
}
