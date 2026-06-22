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

use nmp_core::substrate::KernelEvent;
use nmp_store::{
    EventStore, SearchScopeId, StoredEvent, TextSearchBudget, TextSearchOrder, TextSearchQuery,
    TextSearchStatus,
};
use serde::{Deserialize, Serialize};

use crate::request::{SearchRequest, SearchScope};
use crate::scopes::{SCOPE_LABEL_LONGFORM, SCOPE_LABEL_NOTES, SCOPE_LABEL_PROFILES};

const KIND_NOTE: u32 = 1;

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
        let status = store.text_search_visit(&query, &mut |hit| {
            if let Some(id) = hit.event_id {
                ids.push(id);
            }
            ControlFlow::Continue(())
        });

        for id in ids {
            if let Ok(Some(stored)) = store.peek_by_id(&id) {
                self.insert_stored_cache_hit(&stored);
            }
        }

        match status {
            Ok(s) => s,
            Err(_) => TextSearchStatus::StoreError,
        }
    }

    /// Ingest a single relay-sourced event (NIP-50 fanout). Tagged with the
    /// originating relay url; first arrival wins on a duplicate id.
    pub fn ingest_relay_event(&mut self, event: &KernelEvent, relay_url: String) {
        self.insert_hit(event, SearchHitSource::Relay(relay_url));
    }

    #[must_use]
    pub fn snapshot(&self) -> SearchResultsSnapshot {
        let mut hits: Vec<SearchHit> = self.hits.values().cloned().collect();
        hits.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| a.id.cmp(&b.id)));
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
            SearchScope::Kinds(kinds)
                if kinds.len() == 1 && kinds.contains(&KIND_NOTE) =>
            {
                SCOPE_LABEL_NOTES
            }
            // Multi-kind / custom interests have no single registered FTS
            // scope; the relay path still serves them.
            SearchScope::Kinds(_) | SearchScope::Custom(_) => return None,
        };
        Some(SearchScopeId::from_label(label))
    }

    fn insert_stored_cache_hit(&mut self, stored: &StoredEvent) {
        let event = KernelEvent {
            id: stored.raw.id.clone(),
            author: stored.raw.pubkey.clone(),
            kind: stored.raw.kind,
            created_at: stored.raw.created_at,
            tags: stored.raw.tags.clone(),
            content: stored.raw.content.clone(),
            relay_provenance: Vec::new(),
        };
        self.insert_hit(&event, SearchHitSource::Cache);
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
                relay_provenance: event.relay_provenance.clone(),
                source,
            },
        );
    }
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
