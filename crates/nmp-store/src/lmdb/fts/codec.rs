//! Key + value codecs for the durable FTS sub-dbs (#1811).
//!
//! Split from `fts/mod.rs` for the file-size budget. Pure encode/decode — no
//! LMDB access — so the codec is unit-testable in isolation (`codec_tests`).
//!
//! Layouts (also documented on the `Inner` sub-db fields):
//!   * postings:   `scope(4 BE) || token || 0x00 || !created_at(8 BE) || doc(32)`
//!   * doc-terms:  `doc(32)` → `count(u16 BE) || (scope(4 BE) || len(u16 BE) || token)*`
//!   * term-stats: `scope(4 BE) || token` → `doc-frequency(8 BE)`

#![cfg(feature = "lmdb-backend")]

use crate::text_search::SearchScopeId;
use crate::types::EventId;

/// Delimiter between the token bytes and the ordering suffix in a postings key.
///
/// Safe because the tokenizer emits only `char::is_alphanumeric` runs, so a token
/// never contains a NUL byte — the delimiter therefore unambiguously ends the
/// (variable-length) token segment.
const TOK_DELIM: u8 = 0x00;

/// Length of the fixed postings-key suffix after the token: `0x00 ||
/// rev_created_at(8) || doc_key(32)`.
pub(super) const POSTINGS_SUFFIX_LEN: usize = 1 + 8 + 32;

/// Reverse-ordered created_at so a forward LMDB cursor scan is newest-first.
#[inline]
pub(super) fn rev_created_at(created_at: u64) -> u64 {
    u64::MAX - created_at
}

/// Postings key: `scope(4 BE) || token || 0x00 || rev_created_at(8 BE) || doc(32)`.
pub(super) fn postings_key(scope: SearchScopeId, token: &str, rev: u64, doc: &EventId) -> Vec<u8> {
    let tb = token.as_bytes();
    let mut k = Vec::with_capacity(4 + tb.len() + POSTINGS_SUFFIX_LEN);
    k.extend_from_slice(&scope.discriminant().to_be_bytes());
    k.extend_from_slice(tb);
    k.push(TOK_DELIM);
    k.extend_from_slice(&rev.to_be_bytes());
    k.extend_from_slice(doc);
    k
}

/// Inclusive lower / exclusive upper bounds for the EXACT-token posting range:
/// `[scope||token||0x00 .. scope||token||0x01)`.
pub(super) fn postings_token_bounds(scope: SearchScopeId, token: &str) -> (Vec<u8>, Vec<u8>) {
    let tb = token.as_bytes();
    let mut lo = Vec::with_capacity(4 + tb.len() + 1);
    lo.extend_from_slice(&scope.discriminant().to_be_bytes());
    lo.extend_from_slice(tb);
    lo.push(TOK_DELIM);
    let mut hi = Vec::with_capacity(4 + tb.len() + 1);
    hi.extend_from_slice(&scope.discriminant().to_be_bytes());
    hi.extend_from_slice(tb);
    hi.push(TOK_DELIM + 1);
    (lo, hi)
}

/// Inclusive lower / exclusive upper bounds for a PREFIX (typeahead) scan over
/// every token in `scope` beginning with `prefix`. The upper bound increments the
/// last prefix byte; an all-`0xFF` prefix degenerates to "scan to end of scope".
pub(super) fn postings_prefix_bounds(scope: SearchScopeId, prefix: &str) -> (Vec<u8>, Vec<u8>) {
    let disc = scope.discriminant().to_be_bytes();
    let pb = prefix.as_bytes();
    let mut lo = Vec::with_capacity(4 + pb.len());
    lo.extend_from_slice(&disc);
    lo.extend_from_slice(pb);
    // Exclusive upper: smallest key strictly greater than every key with this
    // prefix. Increment the last non-0xFF byte of (disc||prefix), truncating the
    // tail; if everything is 0xFF the range runs to the very end.
    let mut hi = lo.clone();
    while let Some(last) = hi.last().copied() {
        if last == 0xFF {
            hi.pop();
        } else {
            *hi.last_mut().unwrap() = last + 1;
            break;
        }
    }
    (lo, hi)
}

/// Decode the `(rev_created_at, doc_key)` tail of a postings key whose token byte
/// length is `token_len`. Returns `None` on a malformed key.
pub(super) fn postings_decode_tail(key: &[u8], token_len: usize) -> Option<(u64, EventId)> {
    // Layout: 4 (scope) + token_len + 1 (delim) + 8 (rev) + 32 (doc).
    let want = 4 + token_len + POSTINGS_SUFFIX_LEN;
    if key.len() != want {
        return None;
    }
    let base = 4 + token_len + 1;
    let rev = u64::from_be_bytes(key[base..base + 8].try_into().ok()?);
    let mut doc = [0u8; 32];
    doc.copy_from_slice(&key[base + 8..]);
    Some((rev, doc))
}

/// Term-stats key: `scope(4 BE) || token`.
pub(super) fn term_stats_key(scope: SearchScopeId, token: &str) -> Vec<u8> {
    let tb = token.as_bytes();
    let mut k = Vec::with_capacity(4 + tb.len());
    k.extend_from_slice(&scope.discriminant().to_be_bytes());
    k.extend_from_slice(tb);
    k
}

/// Doc-terms value codec: `count(u16 BE) || (scope(4 BE) || len(u16 BE) || token)*`.
///
/// Keyed by `doc_key` alone (not per scope): a doc lives in exactly the scopes
/// whose `kinds` contain its kind, and removal must drop the doc from EVERY
/// scope, so the value holds the union of `(scope_discriminant, token)` pairs and
/// cleanup is one read.
pub(super) fn encode_doc_terms(entries: &[(u32, String)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + entries.len() * 8);
    out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    for (scope_disc, token) in entries {
        out.extend_from_slice(&scope_disc.to_be_bytes());
        let tb = token.as_bytes();
        out.extend_from_slice(&(tb.len() as u16).to_be_bytes());
        out.extend_from_slice(tb);
    }
    out
}

/// Decode a doc-terms value into `(scope_discriminant, token)` pairs.
pub(super) fn decode_doc_terms(bytes: &[u8]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    if bytes.len() < 2 {
        return out;
    }
    let count = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
    let mut p = 2usize;
    for _ in 0..count {
        if p + 4 + 2 > bytes.len() {
            break;
        }
        let scope_disc = u32::from_be_bytes(bytes[p..p + 4].try_into().unwrap());
        p += 4;
        let len = u16::from_be_bytes([bytes[p], bytes[p + 1]]) as usize;
        p += 2;
        if p + len > bytes.len() {
            break;
        }
        let token = String::from_utf8_lossy(&bytes[p..p + len]).into_owned();
        p += len;
        out.push((scope_disc, token));
    }
    out
}

#[cfg(test)]
mod codec_tests {
    use super::*;

    #[test]
    fn doc_terms_roundtrip() {
        let entries = vec![
            (7u32, "alpha".to_string()),
            (7u32, "beta".to_string()),
            (99u32, "gamma".to_string()),
        ];
        let enc = encode_doc_terms(&entries);
        assert_eq!(decode_doc_terms(&enc), entries);
    }

    #[test]
    fn postings_tail_roundtrips() {
        let scope = SearchScopeId::new(0xDEAD_BEEF, "x");
        let doc = [0xABu8; 32];
        let rev = rev_created_at(1234);
        let k = postings_key(scope, "satoshi", rev, &doc);
        let (got_rev, got_doc) = postings_decode_tail(&k, "satoshi".len()).unwrap();
        assert_eq!(got_rev, rev);
        assert_eq!(got_doc, doc);
    }

    #[test]
    fn prefix_bounds_cover_token() {
        let scope = SearchScopeId::new(1, "x");
        let (lo, hi) = postings_prefix_bounds(scope, "sat");
        let full = postings_key(scope, "satoshi", 0, &[0u8; 32]);
        assert!(lo.as_slice() <= full.as_slice());
        assert!(full.as_slice() < hi.as_slice());
    }
}
