//! Shared full-text tokenizer (issue #1811).
//!
//! Lives in `nmp-store` (not a protocol crate) so **every** backend — the
//! `MemEventStore` here and the Phase-2 LMDB inverted index — runs byte-for-byte
//! the same normalization. Index-time and query-time text MUST flow through the
//! same [`tokenize`] so a token written at ingest is found at query time.
//!
//! # Normalization pipeline (fixed order)
//!
//! 1. **NFKC** — Unicode compatibility-composition, so visually/semantically
//!    equal sequences collapse to one form (e.g. full-width → ASCII, ligatures).
//! 2. **Unicode lowercase** — `str::to_lowercase` (full Unicode case folding,
//!    not ASCII-only), so `"Satoshi"` and `"satoshi"` index identically.
//! 3. **Split on non-alphanumeric** — every maximal run of `char::is_alphanumeric`
//!    is one token; whitespace and punctuation are separators. This is
//!    protocol-noun-free: the store never inspects what the text *means*.
//! 4. **Drop short tokens** — tokens under [`MIN_TOKEN_BYTES`] bytes are dropped
//!    (cuts single-letter / stop-noise without a language-specific stopword list).
//! 5. **Cap token count** — at most [`MAX_TOKENS_PER_DOC`] tokens are kept per
//!    call, bounding index growth for a pathological document (D8).
//!
//! Matching is **token + prefix** (NOT substring, NOT stemming): a query token
//! either equals an indexed token or is a prefix of one (typeahead). See
//! [`is_prefix_match`].

use unicode_normalization::UnicodeNormalization;

/// Bump whenever the tokenization pipeline changes in a way that would make a
/// previously-written index disagree with freshly-tokenized query text. The
/// LMDB backend (Phase 2) persists this alongside its postings so a mismatch
/// triggers a rebuild rather than silently returning wrong hits.
pub const TOKENIZER_VERSION: u32 = 1;

/// Tokens shorter than this many **bytes** are dropped. Two bytes admits short
/// CJK tokens and 2-letter handles while cutting single-character noise.
pub const MIN_TOKEN_BYTES: usize = 2;

/// Hard cap on tokens produced per document (D8 — bound index growth on a
/// pathological body). Excess tokens beyond this count are discarded.
pub const MAX_TOKENS_PER_DOC: usize = 256;

/// Normalize and split `text` into indexable tokens.
///
/// Deterministic and allocation-bounded: returns at most [`MAX_TOKENS_PER_DOC`]
/// tokens, each at least [`MIN_TOKEN_BYTES`] bytes. Used at BOTH index time
/// (per extracted field value) and query time (the raw query string) so the
/// two always agree.
#[must_use]
pub fn tokenize(text: &str) -> Vec<String> {
    // NFKC then full-Unicode lowercase, as one normalized stream.
    let normalized: String = text.nfkc().collect::<String>().to_lowercase();

    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in normalized.chars() {
        if ch.is_alphanumeric() {
            cur.push(ch);
        } else if !cur.is_empty() {
            push_token(&mut out, &mut cur);
            if out.len() >= MAX_TOKENS_PER_DOC {
                return out;
            }
        }
    }
    if !cur.is_empty() && out.len() < MAX_TOKENS_PER_DOC {
        push_token(&mut out, &mut cur);
    }
    out
}

#[inline]
fn push_token(out: &mut Vec<String>, cur: &mut String) {
    if cur.len() >= MIN_TOKEN_BYTES {
        out.push(std::mem::take(cur));
    } else {
        cur.clear();
    }
}

/// Tokenize a query string into its `(exact_terms, prefix_term)` parts.
///
/// All tokens but the last must match an indexed token **exactly** (AND
/// semantics); the trailing token matches by **prefix** (typeahead). Returns
/// `(exact, Some(prefix))` for a non-empty query, or `(vec![], None)` when the
/// query produces no tokens (caller treats that as "match nothing → empty,
/// Complete").
#[must_use]
pub fn split_query_terms(query: &str) -> (Vec<String>, Option<String>) {
    let mut tokens = tokenize(query);
    match tokens.pop() {
        Some(last) => (tokens, Some(last)),
        None => (Vec::new(), None),
    }
}

/// Returns `true` when `candidate` is the prefix term `prefix` or begins with
/// it. Byte-prefix is correct here because both strings already passed through
/// the same NFKC + lowercase normalization in [`tokenize`].
#[must_use]
pub fn is_prefix_match(candidate: &str, prefix: &str) -> bool {
    candidate.as_bytes().starts_with(prefix.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_and_splits_on_punctuation() {
        assert_eq!(tokenize("Hello, World!"), vec!["hello", "world"]);
    }

    #[test]
    fn drops_sub_two_byte_tokens() {
        // "a" is 1 byte → dropped; "to" survives.
        assert_eq!(tokenize("a path to x"), vec!["path", "to"]);
    }

    #[test]
    fn nfkc_folds_fullwidth() {
        // Full-width "ＡＢＣ" → "abc" after NFKC + lowercase.
        assert_eq!(tokenize("ＡＢＣ"), vec!["abc"]);
    }

    #[test]
    fn caps_token_count() {
        let big = (0..1000).map(|i| format!("tok{i}")).collect::<Vec<_>>().join(" ");
        assert_eq!(tokenize(&big).len(), MAX_TOKENS_PER_DOC);
    }

    #[test]
    fn split_query_trailing_is_prefix() {
        let (exact, prefix) = split_query_terms("hello sato");
        assert_eq!(exact, vec!["hello"]);
        assert_eq!(prefix.as_deref(), Some("sato"));
    }

    #[test]
    fn split_query_empty() {
        let (exact, prefix) = split_query_terms("   ,. ");
        assert!(exact.is_empty());
        assert_eq!(prefix, None);
    }

    #[test]
    fn prefix_match_basic() {
        assert!(is_prefix_match("satoshi", "sato"));
        assert!(is_prefix_match("sato", "sato"));
        assert!(!is_prefix_match("nakamoto", "sato"));
    }
}
