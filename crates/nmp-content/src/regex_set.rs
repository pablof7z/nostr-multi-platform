//! Compiled regex set — shared across every `tokenize()` call.
//!
//! All patterns are compiled once via `OnceLock`. Tokenizers collect matches
//! from the full set, sort by byte offset, then walk + classify.
//!
//! Pattern shapes mirror `docs/research/content-rendering/ndk-svelte-registry.md`
//! §4. Anchoring + bounded character classes keep the per-call cost linear
//! in the content length.

use regex::Regex;
use std::sync::OnceLock;

/// One compiled pattern + its kind tag. The kind tag is what the classifier
/// switches on after matches are sorted by offset.
pub(crate) struct TokenPattern {
    pub(crate) re: &'static Regex,
    pub(crate) kind: PatternKind,
}

/// Discriminator emitted by `collect_matches` — the classifier in
/// `tokenizer.rs` uses this to route a match to the right `Segment` variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PatternKind {
    /// `nostr:<bech32>` URI — classified later via `nmp_core::nip21`.
    NostrUri,
    /// `:shortcode:` NIP-30 emoji.
    EmojiShortcode,
    /// `#hashtag` (without `#`).
    Hashtag,
    /// `https?://…` URL — split into Media vs Url by extension lookup.
    Url,
    /// `lnbc…` BOLT-11 invoice.
    Bolt11,
    /// `lno…` BOLT-12 offer.
    Bolt12,
    /// `cashuA…` Cashu token.
    Cashu,
}

fn nostr_uri() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // bech32 alphabet is [a-z0-9] (lowercased post-match by NIP-19); also
        // accept uppercase for tolerance — the decoder normalizes. Anchored
        // on common HRPs only; unknown HRPs fall through as plain text.
        Regex::new(r"(?i)nostr:(npub1|nprofile1|note1|nevent1|naddr1)[ac-hj-np-z02-9]+")
            .expect("nostr URI regex compiles") // doctrine-allow: D6 — compile-time-constant regex literal; a malformed pattern is a programmer error caught on first call, not an operational FFI failure
    })
}

fn emoji_shortcode() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r":([a-zA-Z0-9_+-]+):").expect("emoji shortcode regex compiles")) // doctrine-allow: D6 — compile-time-constant regex literal; malformed pattern is a programmer error, not an operational FFI failure
}

fn hashtag() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Must be preceded by start-of-string or whitespace. The lookbehind
        // is emulated by anchoring on `\b` after capturing whitespace.
        // We keep the leading whitespace out of the match span by using
        // (?:^|\s) and trimming in the classifier.
        Regex::new(r"(?:^|\s)#([\p{L}\p{N}_][\p{L}\p{N}_-]*)").expect("hashtag regex compiles") // doctrine-allow: D6 — compile-time-constant regex literal; malformed pattern is a programmer error, not an operational FFI failure
    })
}

fn url() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Conservative URL set — must start with http(s)://, run until
        // whitespace or common terminators. Trailing punctuation is stripped
        // in the classifier.
        Regex::new(r#"https?://[^\s<>"'`{}|\\^\[\]]+"#).expect("url regex compiles") // doctrine-allow: D6 — compile-time-constant regex literal; malformed pattern is a programmer error, not an operational FFI failure
    })
}

fn bolt11() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\blnbc[0-9a-z]{50,}").expect("bolt11 regex compiles")) // doctrine-allow: D6 — compile-time-constant regex literal; malformed pattern is a programmer error, not an operational FFI failure
}

fn bolt12() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\blno1[0-9a-z]{50,}").expect("bolt12 regex compiles")) // doctrine-allow: D6 — compile-time-constant regex literal; malformed pattern is a programmer error, not an operational FFI failure
}

fn cashu() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\bcashu[AB][A-Za-z0-9_-]{40,}").expect("cashu regex compiles")) // doctrine-allow: D6 — compile-time-constant regex literal; malformed pattern is a programmer error, not an operational FFI failure
}

/// The full ordered pattern set. Order is not load-bearing — `collect_matches`
/// sorts by byte offset and `classify_overlap` resolves any conflicts by
/// preferring the longer / earlier match.
pub(crate) fn patterns() -> [TokenPattern; 7] {
    [
        TokenPattern { re: nostr_uri(), kind: PatternKind::NostrUri },
        TokenPattern { re: bolt11(), kind: PatternKind::Bolt11 },
        TokenPattern { re: bolt12(), kind: PatternKind::Bolt12 },
        TokenPattern { re: cashu(), kind: PatternKind::Cashu },
        TokenPattern { re: url(), kind: PatternKind::Url },
        TokenPattern { re: emoji_shortcode(), kind: PatternKind::EmojiShortcode },
        TokenPattern { re: hashtag(), kind: PatternKind::Hashtag },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nostr_uri_pattern_matches_npub() {
        let m = nostr_uri()
            .find("hello nostr:npub1qqsx0nqrh2asdf rest")
            .expect("matches");
        assert!(m.as_str().starts_with("nostr:npub1"));
    }

    #[test]
    fn url_pattern_matches_https() {
        let m = url().find("see https://example.com/a path").expect("matches");
        assert_eq!(m.as_str(), "https://example.com/a");
    }

    #[test]
    fn hashtag_pattern_captures_tag_without_hash() {
        let cap = hashtag().captures("hello #Nostr there").expect("matches");
        assert_eq!(&cap[1], "Nostr");
    }

    #[test]
    fn bolt11_pattern_matches_lnbc() {
        let invoice = "lnbc100n1pj".repeat(6);
        let line = format!("pay me {invoice}");
        let m = bolt11().find(&line).expect("matches");
        assert!(m.as_str().to_lowercase().starts_with("lnbc"));
    }

    #[test]
    fn cashu_pattern_matches_cashu_token() {
        let tok = format!("cashuA{}", "x".repeat(80));
        let line = format!("gift {tok}");
        let m = cashu().find(&line).expect("matches");
        assert!(m.as_str().starts_with("cashuA"));
    }
}
