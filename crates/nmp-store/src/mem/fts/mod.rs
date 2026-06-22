//! In-memory full-text inverted index for `MemEventStore` (issue #1811).
//!
//! Parity target for the Phase-2 LMDB FTS sub-databases. Maintained
//! symmetrically with the primary event map: every insert/replace/delete site
//! in `insert.rs` / `insert_kind5.rs` / `gc.rs` calls [`fts_index_add`] /
//! [`fts_index_remove`], so a search hit never survives source deletion.
//!
//! # Data structure
//!
//! Per installed [`CompiledIndexSpec`] scope:
//!
//! * `postings: scope -> token -> BTreeSet<(rev_created_at, doc_key)>` — the
//!   inverted index. The first key element is `u64::MAX - created_at` so a
//!   forward `BTreeSet` iteration yields newest-first, and a `range` over a
//!   token prefix gives a prefix scan WITHOUT a full token map walk.
//! * `doc_terms: doc_key -> Vec<token>` — for O(terms) cleanup on removal.
//! * `doc_meta: doc_key -> (kind, created_at)` — for kind/time filtering and
//!   the hit's `created_at` without a primary-map lookup.
//!
//! The store runs only the opaque extractor + the shared tokenizer; it never
//! names a protocol concept (D0).

mod query;

pub(in crate::mem) use query::text_search_visit;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::text_search::{tokenize, CompiledIndexSpec, SearchDocumentKey, SearchScopeId};
use crate::types::StoredEvent;

/// Reverse-ordered created_at so a forward BTree scan is newest-first.
#[inline]
pub(in crate::mem) fn rev_created_at(created_at: u64) -> u64 {
    u64::MAX - created_at
}

/// Per-scope inverted index + cleanup/meta sidecars.
#[derive(Default)]
pub(in crate::mem) struct ScopeIndex {
    /// token → newest-first set of (rev_created_at, doc_key).
    pub(in crate::mem) postings: BTreeMap<String, BTreeSet<(u64, SearchDocumentKey)>>,
    /// doc_key → indexed tokens (for removal cleanup).
    pub(in crate::mem) doc_terms: HashMap<SearchDocumentKey, Vec<String>>,
    /// doc_key → (kind, created_at) for filtering + hit metadata.
    pub(in crate::mem) doc_meta: HashMap<SearchDocumentKey, (u32, u64)>,
}

/// The installed FTS state: the compiled specs + the per-scope indices.
#[derive(Default)]
pub(in crate::mem) struct FtsState {
    /// Installed specs keyed by scope id. Empty until
    /// `install_search_index_specs` runs at composition.
    pub(in crate::mem) specs: HashMap<SearchScopeId, Arc<CompiledIndexSpec>>,
    /// Per-scope inverted index.
    pub(in crate::mem) indices: HashMap<SearchScopeId, ScopeIndex>,
}

impl FtsState {
    /// Install (replace) the compiled spec set. Clears any prior index state so
    /// a re-install rebuilds from scratch (composition runs once in production).
    pub(in crate::mem) fn install(&mut self, specs: Vec<CompiledIndexSpec>) {
        self.specs.clear();
        self.indices.clear();
        for spec in specs {
            let id = spec.scope_id;
            self.indices.entry(id).or_default();
            self.specs.insert(id, Arc::new(spec));
        }
    }
}

/// Index `stored` into every installed scope whose kinds include the event's
/// kind. Called at each insert/replace site AFTER the event is in `st.events`.
///
/// Private/encrypted kinds are excluded by construction: such kinds are never
/// placed in a public scope's `kinds` set by the `nmp-core` compiler, and a
/// `local_only_private` scope's documents are flagged so search never serves
/// them. The store runs only the opaque extractor + shared tokenizer.
pub(in crate::mem) fn fts_index_add(st: &mut super::MemState, stored: &StoredEvent) {
    if st.fts.specs.is_empty() {
        return;
    }
    let kind = stored.raw.kind;
    let created_at = stored.raw.created_at;
    let Some(id_bytes) = stored.raw.id_bytes() else {
        return;
    };
    let doc = SearchDocumentKey::from_event_id(id_bytes);

    // Collect work first to avoid borrowing `st.fts` mutably while reading specs.
    let scope_ids: Vec<SearchScopeId> = st
        .fts
        .specs
        .values()
        .filter(|spec| spec.kinds.contains(&kind))
        .map(|spec| spec.scope_id)
        .collect();

    for scope_id in scope_ids {
        let Some(spec) = st.fts.specs.get(&scope_id).cloned() else {
            continue;
        };
        let pairs = (spec.extract)(stored);
        let mut tokens: BTreeSet<String> = BTreeSet::new();
        for (_field, text) in pairs {
            for tok in tokenize(&text) {
                tokens.insert(tok);
            }
        }
        if tokens.is_empty() {
            continue;
        }
        let index = st.fts.indices.entry(scope_id).or_default();
        // Idempotent: remove any prior doc rows for this scope first.
        remove_doc_from_scope(index, doc);
        let rev = rev_created_at(created_at);
        let mut term_list: Vec<String> = Vec::with_capacity(tokens.len());
        for tok in tokens {
            index
                .postings
                .entry(tok.clone())
                .or_default()
                .insert((rev, doc));
            term_list.push(tok);
        }
        index.doc_terms.insert(doc, term_list);
        index.doc_meta.insert(doc, (kind, created_at));
    }
}

/// Remove a document (by hex event id) from every scope's index.
/// Called at each delete/replace/GC site, mirroring `relay_index_remove`.
pub(in crate::mem) fn fts_index_remove(st: &mut super::MemState, id_hex: &str) {
    if st.fts.indices.is_empty() {
        return;
    }
    let Some(doc) = hex_to_doc(id_hex) else {
        return;
    };
    for index in st.fts.indices.values_mut() {
        remove_doc_from_scope(index, doc);
    }
}

fn remove_doc_from_scope(index: &mut ScopeIndex, doc: SearchDocumentKey) {
    index.doc_meta.remove(&doc);
    let Some(terms) = index.doc_terms.remove(&doc) else {
        return;
    };
    // We need the rev key to drop the exact posting; recompute from any
    // remaining meta is impossible after removal, so scan the token's set for
    // the doc. The set is small per token; this stays O(terms * postings/token).
    for tok in terms {
        if let Some(set) = index.postings.get_mut(&tok) {
            set.retain(|(_, d)| *d != doc);
            if set.is_empty() {
                index.postings.remove(&tok);
            }
        }
    }
}

fn hex_to_doc(id_hex: &str) -> Option<SearchDocumentKey> {
    crate::types::hex_to_event_id(id_hex).map(SearchDocumentKey::from_event_id)
}
