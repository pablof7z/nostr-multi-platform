//! Bounded, deduplicating NIP-50 result projection.
//!
//! Cache hits are sourced from the store's full-text-search seam
//! ([`nmp_store::EventStore::text_search_visit`], issue #1811) — the same
//! tokenizer that indexed the event at ingest matches the query, so the ad-hoc
//! linear substring scan is retired. Relay hits arrive event-by-event from the
//! NIP-50 fanout. First arrival wins on a duplicate event id, so a cache hit
//! that lands before the relay echo is tagged [`SearchHitSource::Cache`].

use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

use nmp_core::slots::relay_provenance_for_event;
use nmp_core::substrate::KernelEvent;
use nmp_kinds::{KIND_LONG_FORM_ARTICLE, KIND_PROFILE_METADATA, KIND_SHORT_TEXT_NOTE};
use nmp_store::{
    is_prefix_match, split_query_terms, tokenize, EventStore, SearchScopeId, StoreQuery,
    StoredEvent, TextSearchBudget, TextSearchOrder, TextSearchQuery, TextSearchStatus,
};
use serde::{Deserialize, Serialize};

use crate::request::{SearchRequest, SearchScope};
use crate::scopes::{SCOPE_LABEL_LONGFORM, SCOPE_LABEL_NOTES, SCOPE_LABEL_PROFILES};

const CACHE_FALLBACK_SCAN_LIMIT: usize = 512;

fn is_short_text_note_scope(kinds: &BTreeSet<u32>) -> bool {
    kinds.len() == 1 && kinds.contains(&KIND_SHORT_TEXT_NOTE)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchHitSource {
    Cache,
    Relay(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub author: String,
    pub kind: u32,
    pub created_at: u64,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    pub relay_provenance: Vec<String>,
    pub source: SearchHitSource,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchResultsSnapshot {
    pub hits: Vec<SearchHit>,
}

pub struct SearchResultsProjection {
    request: SearchRequest,
    hits: BTreeMap<String, SearchHit>,
}

impl SearchResultsProjection {
    #[must_use]
    pub fn new(request: SearchRequest) -> Self {
        Self {
            request,
            hits: BTreeMap::new(),
        }
    }

    /// Pull cache hits from the store's FTS index for this request's scope.
    ///
    /// Runs one bounded [`EventStore::text_search_visit`] over the scope that
    /// the request maps to, fetches each matching event with a pure point-read
    /// (`peek_by_id`, no LRU stamping), and ingests it tagged
    /// [`SearchHitSource::Cache`]. Returns the visit's [`TextSearchStatus`] so
    /// the caller can surface "partial / building / unsupported" in search UI.
    /// A scope with no FTS mapping (e.g. a free-form `Custom` interest) is
    /// reported as [`TextSearchStatus::Unsupported`] and ingests nothing.
    pub fn ingest_cache_from_store(&mut self, store: &dyn EventStore) -> TextSearchStatus {
        let Some(scope) = self.cache_scope_id() else {
            return TextSearchStatus::Unsupported;
        };
        let query = TextSearchQuery {
            scope,
            query: self.request.query.clone(),
            kinds: BTreeSet::new(),
            since: None,
            until: None,
            limit: self.request.max_hits,
            order: TextSearchOrder::NewestFirst,
            budget: TextSearchBudget::default(),
        };

        let mut ids: Vec<[u8; 32]> = Vec::new();
        let status = match store.text_search_visit(&query, &mut |hit| {
            if let Some(id) = hit.event_id {
                ids.push(id);
            }
            ControlFlow::Continue(())
        }) {
            Ok(status) => status,
            Err(_) => TextSearchStatus::StoreError,
        };

        for id in ids {
            if let Ok(Some(stored)) = store.peek_by_id(&id) {
                self.insert_stored_cache_hit(&stored, store);
            }
        }

        if status == TextSearchStatus::Unsupported {
            self.ingest_cache_from_scope_scan(store);
            return TextSearchStatus::Complete;
        }

        status
    }

    /// Ingest a single relay-sourced event (NIP-50 fanout). Tagged with the
    /// originating relay url; first arrival wins on a duplicate id.
    pub fn ingest_relay_event(&mut self, event: &KernelEvent, relay_url: String) {
        self.insert_hit(event, SearchHitSource::Relay(relay_url));
    }

    #[must_use]
    pub fn snapshot(&self) -> SearchResultsSnapshot {
        let mut hits: Vec<SearchHit> = self.hits.values().cloned().collect();
        hits.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        SearchResultsSnapshot { hits }
    }

    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or_else(|_| serde_json::json!({ "hits": [] }))
    }

    /// Map the request scope to the store FTS scope id, when one exists.
    fn cache_scope_id(&self) -> Option<SearchScopeId> {
        let label = match &self.request.scope {
            SearchScope::Users => SCOPE_LABEL_PROFILES,
            SearchScope::LongForm => SCOPE_LABEL_LONGFORM,
            SearchScope::Kinds(kinds) if is_short_text_note_scope(kinds) => SCOPE_LABEL_NOTES,
            // Multi-kind / custom interests have no single registered FTS
            // scope; the relay path still serves them.
            SearchScope::Kinds(_) | SearchScope::Custom(_) => return None,
        };
        Some(SearchScopeId::from_label(label))
    }

    fn insert_stored_cache_hit(&mut self, stored: &StoredEvent, store: &dyn EventStore) {
        let event = KernelEvent {
            id: stored.raw.id.clone(),
            author: stored.raw.pubkey.clone(),
            kind: stored.raw.kind,
            created_at: stored.raw.created_at,
            tags: stored.raw.tags.clone(),
            content: stored.raw.content.clone(),
            relay_provenance: relay_provenance_for_event(store, &stored.raw.id),
        };
        self.insert_hit(&event, SearchHitSource::Cache);
    }

    fn ingest_cache_from_scope_scan(&mut self, store: &dyn EventStore) {
        let Some(kinds) = self.cache_scan_kinds() else {
            return;
        };
        let query = StoreQuery::KindTime {
            kinds,
            since: None,
            until: None,
        };
        let mut matches = Vec::new();
        let _ = store.query_visit(&query, CACHE_FALLBACK_SCAN_LIMIT, &mut |stored| {
            if self.stored_event_matches_query(stored) {
                matches.push(stored.clone());
            }
            if matches.len() >= self.request.max_hits {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        for stored in matches {
            self.insert_stored_cache_hit(&stored, store);
        }
    }

    fn cache_scan_kinds(&self) -> Option<Vec<u32>> {
        match &self.request.scope {
            SearchScope::Users => Some(vec![KIND_PROFILE_METADATA]),
            SearchScope::LongForm => Some(vec![KIND_LONG_FORM_ARTICLE]),
            SearchScope::Kinds(kinds) if is_short_text_note_scope(kinds) => {
                Some(vec![KIND_SHORT_TEXT_NOTE])
            }
            SearchScope::Kinds(_) | SearchScope::Custom(_) => None,
        }
    }

    fn stored_event_matches_query(&self, stored: &StoredEvent) -> bool {
        let haystack = match &self.request.scope {
            SearchScope::Users => profile_search_text(&stored.raw.content),
            SearchScope::LongForm => longform_search_text(&stored.raw.content, &stored.raw.tags),
            SearchScope::Kinds(kinds) if is_short_text_note_scope(kinds) => {
                stored.raw.content.clone()
            }
            SearchScope::Kinds(_) | SearchScope::Custom(_) => String::new(),
        };
        token_query_matches(&haystack, &self.request.query)
    }

    fn insert_hit(&mut self, event: &KernelEvent, source: SearchHitSource) {
        if self.hits.contains_key(&event.id) {
            return;
        }
        if self.hits.len() >= self.request.max_hits {
            return;
        }
        let shape = self.request.interest_shape();
        if !shape.matches_event_with_id(
            &event.id,
            &event.author,
            event.kind,
            event.created_at,
            &event.tags,
        ) {
            return;
        }
        self.hits.insert(
            event.id.clone(),
            SearchHit {
                id: event.id.clone(),
                author: event.author.clone(),
                kind: event.kind,
                created_at: event.created_at,
                content: event.content.clone(),
                tags: event.tags.clone(),
                relay_provenance: event.received_from_relays(),
                source,
            },
        );
    }
}

fn token_query_matches(text: &str, query: &str) -> bool {
    let doc_tokens = tokenize(text);
    if doc_tokens.is_empty() {
        return false;
    }
    let (exact_terms, prefix_term) = split_query_terms(query);
    if exact_terms.is_empty() && prefix_term.is_none() {
        return false;
    }
    exact_terms
        .iter()
        .all(|term| doc_tokens.iter().any(|candidate| candidate == term))
        && prefix_term.as_deref().is_none_or(|prefix| {
            doc_tokens
                .iter()
                .any(|candidate| is_prefix_match(candidate, prefix))
        })
}

fn profile_search_text(content: &str) -> String {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(content)
    else {
        return String::new();
    };
    ["name", "display_name", "displayName", "nip05", "about"]
        .into_iter()
        .filter_map(|key| map.get(key).and_then(|value| value.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn longform_search_text(content: &str, tags: &[Vec<String>]) -> String {
    let mut parts = Vec::new();
    for key in ["title", "summary"] {
        if let Some(value) = first_tag_value(tags, key) {
            parts.push(value);
        }
    }
    parts.push(content.to_string());
    parts.join(" ")
}

fn first_tag_value(tags: &[Vec<String>], name: &str) -> Option<String> {
    tags.iter()
        .find(|tag| tag.first().is_some_and(|key| key == name))
        .and_then(|tag| tag.get(1))
        .filter(|value| !value.is_empty())
        .cloned()
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
