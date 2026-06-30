//! `text_search_visit` for the durable LMDB inverted index (#1811).
//!
//! Plan (mirrors `mem/fts/query.rs` semantics, AND + trailing-prefix typeahead,
//! newest-first, bounded — never a broad event-store scan):
//!
//! 1. `split_query_terms` → exact terms + one trailing prefix term.
//! 2. Seed the scan from the trailing prefix term's posting range (a prefix range
//!    scan over `nmp-fts-postings`), collecting candidate `(rev, doc)` newest-
//!    first, bounded by `budget.max_docs_scanned`.
//! 3. AND-intersect each exact term, processed RAREST-FIRST (via
//!    `nmp-fts-term-stats` doc-frequency) so the candidate set shrinks fastest;
//!    an exact term with no postings short-circuits to empty.
//! 4. Order candidates newest-first, apply kind/since/until filters, emit up to
//!    `limit` / `budget.max_matches`.
//!
//! Unknown / unregistered scope → `Unsupported` (parity with the trait default).

#![cfg(feature = "lmdb-backend")]

use std::collections::HashMap;
use std::ops::{Bound, ControlFlow};

use super::codec::{postings_decode_tail, postings_key, postings_prefix_bounds};
use super::term_doc_frequency;
use crate::lmdb::Inner;
use crate::text_search::tokenizer::{is_prefix_match, split_query_terms};
use crate::text_search::{
    SearchDocumentKey, SearchScore, TextSearchHit, TextSearchOrder, TextSearchQuery,
    TextSearchStatus,
};
use crate::StoreError;

pub(in crate::lmdb) fn text_search_visit(
    inner: &Inner,
    query: &TextSearchQuery,
    visitor: &mut dyn FnMut(TextSearchHit) -> ControlFlow<()>,
) -> Result<TextSearchStatus, StoreError> {
    // Unknown scope → Unsupported. A scope is "known" iff it is in the installed
    // spec set (matches mem, which keys its indices by installed scope).
    {
        let specs = match inner.fts_specs.read() {
            Ok(g) => g,
            Err(_) => return Ok(TextSearchStatus::Unsupported),
        };
        if specs.is_empty() {
            return Ok(TextSearchStatus::Unsupported);
        }
        if !specs.iter().any(|s| s.scope_id == query.scope) {
            return Ok(TextSearchStatus::Unsupported);
        }
    }

    let (exact_terms, prefix_term) = split_query_terms(&query.query);
    let Some(prefix) = prefix_term else {
        // Empty query (no tokens) → Complete, no hits, no scan.
        return Ok(TextSearchStatus::Complete);
    };

    let txn = inner
        .env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("fts read_txn: {e}")))?;

    let mut docs_scanned = 0usize;
    let (mut candidates, budget_exhausted) =
        collect_candidates(inner, &txn, query, &exact_terms, &prefix, &mut docs_scanned)?;

    order_candidates(&mut candidates, query);
    emit(inner, &txn, candidates, query, visitor, budget_exhausted)
}

/// Build the newest-first candidate `(rev_created_at, doc)` list. Returns
/// `(candidates, budget_exhausted)`.
fn collect_candidates(
    inner: &Inner,
    txn: &heed::RoTxn,
    query: &TextSearchQuery,
    exact_terms: &[String],
    prefix: &str,
    docs_scanned: &mut usize,
) -> Result<(Vec<(u64, SearchDocumentKey)>, bool), StoreError> {
    // 1. Prefix term → union of docs under every token in this scope starting
    //    with `prefix`. We track the BEST (smallest rev = newest) per doc so the
    //    hit's created_at is the document's own, deterministically. The scan is
    //    bounded by `budget.max_docs_scanned`; on exhaustion we DON'T early-return
    //    the raw prefix docs — we fall through to the AND filter below so a
    //    Partial result is never a superset of the true matches (#1882/#2).
    let mut prefix_docs: HashMap<SearchDocumentKey, u64> = HashMap::new();
    let mut budget_exhausted = false;
    let (lo, hi) = postings_prefix_bounds(query.scope, prefix);
    let range = (
        Bound::Included(lo.as_slice()),
        Bound::Excluded(hi.as_slice()),
    );
    let prefix_disc_len = 4 + prefix.len();
    'scan: for entry in inner
        .fts_postings
        .range(txn, &range)
        .map_err(|e| StoreError::Io(format!("fts prefix range: {e}")))?
    {
        let (k, _) = entry.map_err(|e| StoreError::Io(format!("fts prefix step: {e}")))?;
        // The token length is variable; the token runs from byte 4 up to the
        // 0x00 delimiter. Locate the delimiter that precedes the fixed suffix.
        let Some(token_len) = token_len_of(k) else {
            continue;
        };
        // Guard: the scanned key's token must actually start with the prefix
        // (range upper bound is a byte-increment, which is exact for our keys,
        // but recheck defensively).
        if k.len() < prefix_disc_len || !is_prefix_match_key(k, prefix) {
            continue;
        }
        if let Some((rev, doc_bytes)) = postings_decode_tail(k, token_len) {
            let doc = SearchDocumentKey(doc_bytes);
            prefix_docs
                .entry(doc)
                .and_modify(|r| {
                    if rev < *r {
                        *r = rev;
                    }
                })
                .or_insert(rev);
            *docs_scanned += 1;
            if *docs_scanned >= query.budget.max_docs_scanned {
                budget_exhausted = true;
                break 'scan;
            }
        }
    }

    if prefix_docs.is_empty() {
        return Ok((Vec::new(), budget_exhausted));
    }

    // 2. AND with each exact term, RAREST-FIRST so the candidate set shrinks
    //    fastest. We filter the ALREADY-collected (bounded) candidate set via a
    //    point look-up per surviving candidate — one `get` keyed by the doc's own
    //    rev — instead of materializing the exact term's (potentially unbounded)
    //    posting list (#1882/#3, D8). Each posting key for a given doc shares that
    //    doc's single rev_created_at, so the point look-up is exact.
    let mut ordered_terms: Vec<(u64, &String)> = Vec::with_capacity(exact_terms.len());
    for t in exact_terms {
        let df = term_doc_frequency(inner, txn, query.scope, t)?;
        ordered_terms.push((df, t));
    }
    ordered_terms.sort_by_key(|(df, _)| *df);

    for (df, term) in ordered_terms {
        if df == u64::MAX {
            // No postings for this exact term → no results possible.
            return Ok((Vec::new(), budget_exhausted));
        }
        let mut kept: HashMap<SearchDocumentKey, u64> = HashMap::with_capacity(prefix_docs.len());
        for (doc, rev) in prefix_docs {
            let key = postings_key(query.scope, term, rev, &doc.0);
            let present = inner
                .fts_postings
                .get(txn, &key)
                .map_err(|e| StoreError::Io(format!("fts and-filter get: {e}")))?
                .is_some();
            if present {
                kept.insert(doc, rev);
            }
        }
        prefix_docs = kept;
        if prefix_docs.is_empty() {
            return Ok((Vec::new(), budget_exhausted));
        }
    }

    // Materialize survivors with their own (newest) rev from the prefix pass.
    let out: Vec<(u64, SearchDocumentKey)> = prefix_docs.into_iter().map(|(d, r)| (r, d)).collect();
    Ok((out, budget_exhausted))
}

/// Locate the token byte length in a postings key: the bytes between offset 4
/// and the `0x00` delimiter that precedes the fixed 41-byte suffix.
fn token_len_of(key: &[u8]) -> Option<usize> {
    // Fixed suffix = 1 (delim) + 8 (rev) + 32 (doc) = 41. token_len = total - 4 - 41.
    if key.len() < 4 + super::codec::POSTINGS_SUFFIX_LEN {
        return None;
    }
    Some(key.len() - 4 - super::codec::POSTINGS_SUFFIX_LEN)
}

/// Does the postings key's token segment start with `prefix`?
fn is_prefix_match_key(key: &[u8], prefix: &str) -> bool {
    if key.len() < 4 + prefix.len() {
        return false;
    }
    // Token starts at byte 4; verify the prefix bytes match, then confirm the
    // candidate token (up to the delimiter) is a real prefix match.
    let token_len = match token_len_of(key) {
        Some(l) => l,
        None => return false,
    };
    let token = &key[4..4 + token_len];
    match std::str::from_utf8(token) {
        Ok(s) => is_prefix_match(s, prefix),
        Err(_) => token.starts_with(prefix.as_bytes()),
    }
}

/// Order candidates (ascending rev == descending created_at).
fn order_candidates(candidates: &mut [(u64, SearchDocumentKey)], query: &TextSearchQuery) {
    match query.order {
        TextSearchOrder::NewestFirst => {
            candidates.sort_unstable_by_key(|(rev, doc)| (*rev, *doc));
        }
        TextSearchOrder::Relevance => {
            // Phase-1: no term-frequency data is stored in the posting index,
            // so true TF-IDF ranking is deferred to Phase-2. Use recency as a
            // proxy (newest-first), matching mem-backend parity. The variant is
            // kept frozen so Phase-2 can wire scoring without an API change.
            // NOTE: callers receive recency order, not relevance order.
            candidates.sort_unstable_by_key(|(rev, doc)| (*rev, *doc));
        }
    }
}

/// Emit candidates newest-first up to limit/budget, applying kind/time filters.
fn emit(
    inner: &Inner,
    txn: &heed::RoTxn,
    candidates: Vec<(u64, SearchDocumentKey)>,
    query: &TextSearchQuery,
    visitor: &mut dyn FnMut(TextSearchHit) -> ControlFlow<()>,
    mut budget_exhausted: bool,
) -> Result<TextSearchStatus, StoreError> {
    let mut emitted = 0usize;
    let mut hit_limit = false;

    for (rev, doc) in candidates {
        let created_at = u64::MAX - rev;
        // Time filters straight off the posting's (newest-first) created_at.
        if let Some(since) = query.since {
            if created_at < since {
                continue;
            }
        }
        if let Some(until) = query.until {
            if created_at > until {
                continue;
            }
        }
        // Kind narrowing: when the caller restricts `kinds`, point-look-up the
        // document's kind from the primary store (bounded: at most one read per
        // surviving candidate, capped by limit/budget). The posting key stays
        // compact — kind is intentionally not encoded into it.
        if !query.kinds.is_empty() {
            let kind = match inner
                .lmdb
                .get_event_by_id(txn, &doc.0)
                .map_err(|e| StoreError::Io(format!("fts kind lookup: {e}")))?
            {
                Some(ev) => ev.into_owned().kind.as_u16() as u32,
                None => continue, // doc gone from primary store — skip
            };
            if !query.kinds.contains(&kind) {
                continue;
            }
        }
        let hit = TextSearchHit {
            doc,
            event_id: Some(doc.0),
            created_at,
            score: SearchScore(1),
        };
        if emitted >= query.limit {
            hit_limit = true;
            break;
        }
        if emitted >= query.budget.max_matches {
            budget_exhausted = true;
            break;
        }
        emitted += 1;
        if let ControlFlow::Break(()) = visitor(hit) {
            return Ok(TextSearchStatus::Complete);
        }
    }

    if budget_exhausted {
        Ok(TextSearchStatus::Partial {
            budget_exhausted: true,
        })
    } else if hit_limit {
        Ok(TextSearchStatus::Partial {
            budget_exhausted: false,
        })
    } else {
        Ok(TextSearchStatus::Complete)
    }
}
