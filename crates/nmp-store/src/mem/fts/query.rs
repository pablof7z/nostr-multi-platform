//! `text_search_visit` query execution for the in-memory FTS index (#1811).
//!
//! Token + prefix matching with AND semantics: all but the trailing query token
//! must match an indexed token EXACTLY; the trailing token matches by prefix
//! (typeahead). Newest-first ordering comes for free from the reverse-keyed
//! postings sets. Bounded by `query.budget` and `query.limit` — no hidden full
//! scan: when the query produces no tokens we return `Complete`/empty without
//! touching the index.

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use crate::text_search::{
    is_prefix_match, split_query_terms, SearchDocumentKey, SearchScore, TextSearchHit,
    TextSearchOrder, TextSearchQuery, TextSearchStatus,
};
use crate::MemEventStore;
use crate::StoreError;

use super::ScopeIndex;

pub(in crate::mem) fn text_search_visit(
    store: &MemEventStore,
    query: &TextSearchQuery,
    visitor: &mut dyn FnMut(TextSearchHit) -> ControlFlow<()>,
) -> Result<TextSearchStatus, StoreError> {
    let st = store.lock()?;

    // Unknown / unregistered scope → Unsupported (parity with the trait default).
    let Some(index) = st.fts.indices.get(&query.scope) else {
        return Ok(TextSearchStatus::Unsupported);
    };

    let (exact_terms, prefix_term) = split_query_terms(&query.query);
    let Some(prefix) = prefix_term else {
        // Empty query (no tokens) → complete, no hits, no scan.
        return Ok(TextSearchStatus::Complete);
    };

    // Candidate doc set = intersection of (exact-term posting docs) ∩
    // (union of docs under any token with `prefix`). Build the candidate set,
    // then filter by kind/time and emit newest-first within the limit/budget.
    let mut docs_scanned = 0usize;
    let candidates = match collect_candidates(index, &exact_terms, &prefix, query, &mut docs_scanned)
    {
        Candidates::Set(set) => set,
        Candidates::BudgetExhausted(set) => {
            // We hit the scan budget while gathering candidates.
            return emit(
                index,
                set,
                query,
                visitor,
                /* budget_exhausted = */ true,
            );
        }
    };

    emit(index, candidates, query, visitor, false)
}

enum Candidates {
    Set(Vec<(u64, SearchDocumentKey)>),
    BudgetExhausted(Vec<(u64, SearchDocumentKey)>),
}

/// Build the newest-first candidate `(rev_created_at, doc)` list.
fn collect_candidates(
    index: &ScopeIndex,
    exact_terms: &[String],
    prefix: &str,
    query: &TextSearchQuery,
    docs_scanned: &mut usize,
) -> Candidates {
    // 1. Prefix term → union of docs under every token starting with `prefix`.
    //    BTreeMap range from the prefix gives us only the matching tokens.
    let mut prefix_docs: BTreeSet<SearchDocumentKey> = BTreeSet::new();
    for (tok, set) in index.postings.range(prefix.to_string()..) {
        if !is_prefix_match(tok, prefix) {
            break; // sorted: first non-prefix token ends the range
        }
        for (_rev, doc) in set {
            prefix_docs.insert(*doc);
            *docs_scanned += 1;
            if *docs_scanned >= query.budget.max_docs_scanned {
                return Candidates::BudgetExhausted(order_docs(index, prefix_docs, query));
            }
        }
    }

    // 2. AND with each exact term's doc set.
    let mut acc = prefix_docs;
    for term in exact_terms {
        let Some(set) = index.postings.get(term) else {
            // An exact term with no postings → no results possible.
            return Candidates::Set(Vec::new());
        };
        let term_docs: BTreeSet<SearchDocumentKey> = set.iter().map(|(_, d)| *d).collect();
        acc = acc.intersection(&term_docs).copied().collect();
        if acc.is_empty() {
            return Candidates::Set(Vec::new());
        }
    }

    Candidates::Set(order_docs(index, acc, query))
}

/// Materialize a doc set into a newest-first `(rev_created_at, doc)` vec,
/// applying kind/time filters from `query`.
fn order_docs(
    index: &ScopeIndex,
    docs: BTreeSet<SearchDocumentKey>,
    query: &TextSearchQuery,
) -> Vec<(u64, SearchDocumentKey)> {
    let mut out: Vec<(u64, SearchDocumentKey)> = docs
        .into_iter()
        .filter_map(|doc| {
            let (kind, created_at) = *index.doc_meta.get(&doc)?;
            if !query.kinds.is_empty() && !query.kinds.contains(&kind) {
                return None;
            }
            if let Some(since) = query.since {
                if created_at < since {
                    return None;
                }
            }
            if let Some(until) = query.until {
                if created_at > until {
                    return None;
                }
            }
            Some((super::rev_created_at(created_at), doc))
        })
        .collect();
    out.sort_unstable(); // ascending rev_created_at == descending created_at
    out
}

/// Emit candidates newest-first up to limit/budget, invoking `visitor`.
fn emit(
    index: &ScopeIndex,
    candidates: Vec<(u64, SearchDocumentKey)>,
    query: &TextSearchQuery,
    visitor: &mut dyn FnMut(TextSearchHit) -> ControlFlow<()>,
    mut budget_exhausted: bool,
) -> Result<TextSearchStatus, StoreError> {
    // Relevance order is a simple match-count today (token+prefix). Sort by
    // score desc when requested, else keep the newest-first vec.
    let mut emitted = 0usize;
    let mut hit_limit = false;

    let ordered: Vec<(u64, SearchDocumentKey)> = match query.order {
        TextSearchOrder::NewestFirst => candidates,
        TextSearchOrder::Relevance => {
            // Phase-1 relevance == created_at recency tiebreak only (no term
            // frequency yet). Keep newest-first; the type is frozen so Phase-2
            // can refine without an API change.
            candidates
        }
    };

    for (rev, doc) in ordered {
        if emitted >= query.limit {
            hit_limit = true;
            break;
        }
        if emitted >= query.budget.max_matches {
            budget_exhausted = true;
            break;
        }
        let created_at = u64::MAX - rev;
        let hit = TextSearchHit {
            doc,
            event_id: Some(doc.0),
            created_at,
            score: SearchScore(1),
        };
        emitted += 1;
        if let ControlFlow::Break(()) = visitor(hit) {
            // Caller stopped early — treat as Complete for what it asked.
            let _ = index;
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
