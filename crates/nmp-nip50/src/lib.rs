//! `nmp-nip50` — NIP-50 search request and result projection primitives.
//!
//! The planner/core substrate owns only the generic `search` filter field and
//! wire serialization. This crate owns NIP-50 query scopes and bounded
//! deduplicating result projection.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::planner::{bounded_search_query, InterestShape};
use nmp_core::substrate::{KernelEvent, ViewDependencies};
use nmp_kinds::KIND_PROFILE_METADATA;
use serde::{Deserialize, Serialize};

pub const KIND_LONG_FORM: u32 = 30_023;
pub const DEFAULT_MAX_SEARCH_HITS: usize = 200;
pub const HARD_MAX_SEARCH_HITS: usize = 500;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchScope {
    Users,
    LongForm,
    Kinds(BTreeSet<u32>),
    Custom(InterestShape),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchTargets {
    UserPreferred,
    Explicit(Vec<String>),
    AppDefault,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub scope: SearchScope,
    pub targets: SearchTargets,
    pub max_hits: usize,
}

impl SearchRequest {
    #[must_use]
    pub fn new(
        query: &str,
        scope: SearchScope,
        targets: SearchTargets,
        max_hits: Option<usize>,
    ) -> Option<Self> {
        Some(Self {
            query: bounded_search_query(query)?,
            scope,
            targets,
            max_hits: max_hits
                .unwrap_or(DEFAULT_MAX_SEARCH_HITS)
                .min(HARD_MAX_SEARCH_HITS),
        })
    }

    #[must_use]
    pub fn interest_shape(&self) -> InterestShape {
        let mut shape = match &self.scope {
            SearchScope::Users => InterestShape {
                kinds: BTreeSet::from([KIND_PROFILE_METADATA]),
                ..Default::default()
            },
            SearchScope::LongForm => InterestShape {
                kinds: BTreeSet::from([KIND_LONG_FORM]),
                ..Default::default()
            },
            SearchScope::Kinds(kinds) => InterestShape {
                kinds: kinds.clone(),
                ..Default::default()
            },
            SearchScope::Custom(shape) => shape.clone(),
        };
        shape.search = Some(self.query.clone());
        shape.limit = Some(self.max_hits.min(u32::MAX as usize) as u32);
        shape
    }

    #[must_use]
    pub fn view_dependencies(&self) -> Option<ViewDependencies> {
        let kinds: Vec<u32> = match &self.scope {
            SearchScope::Users => vec![KIND_PROFILE_METADATA],
            SearchScope::LongForm => vec![KIND_LONG_FORM],
            SearchScope::Kinds(kinds) => kinds.iter().copied().collect(),
            SearchScope::Custom(_) => return None,
        };
        Some(ViewDependencies {
            kinds,
            search: Some(self.query.clone()),
            limit: Some(self.max_hits.min(u32::MAX as usize) as u32),
            ..Default::default()
        })
    }
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

    pub fn ingest_cache_event(&mut self, event: &KernelEvent) {
        if self.matches_local_query(event) {
            self.insert_hit(event, SearchHitSource::Cache);
        }
    }

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

    fn matches_local_query(&self, event: &KernelEvent) -> bool {
        let needle = self.request.query.to_lowercase();
        event.content.to_lowercase().contains(&needle)
            || event
                .tags
                .iter()
                .flatten()
                .any(|cell| cell.to_lowercase().contains(&needle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::EventId;
    use nmp_kinds::KIND_SHORT_TEXT_NOTE;

    fn event(id: &str, kind: u32, content: &str, created_at: u64) -> KernelEvent {
        KernelEvent {
            id: EventId::from(id.to_string()),
            author: "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee".to_string(),
            kind,
            created_at,
            tags: Vec::new(),
            content: content.to_string(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn request_builds_bounded_search_shape() {
        let request = SearchRequest::new(
            "  nostr rust  ",
            SearchScope::Kinds(BTreeSet::from([KIND_SHORT_TEXT_NOTE])),
            SearchTargets::UserPreferred,
            Some(10),
        )
        .expect("query");

        let shape = request.interest_shape();
        assert_eq!(shape.search.as_deref(), Some("nostr rust"));
        assert_eq!(shape.kinds, BTreeSet::from([KIND_SHORT_TEXT_NOTE]));
        assert_eq!(shape.limit, Some(10));
    }

    #[test]
    fn request_rejects_empty_query_and_caps_hits() {
        assert!(SearchRequest::new(
            "   ",
            SearchScope::Users,
            SearchTargets::UserPreferred,
            None
        )
        .is_none());

        let request = SearchRequest::new(
            "nostr",
            SearchScope::Users,
            SearchTargets::UserPreferred,
            Some(HARD_MAX_SEARCH_HITS + 1),
        )
        .expect("query");
        assert_eq!(request.max_hits, HARD_MAX_SEARCH_HITS);
    }

    #[test]
    fn projection_deduplicates_first_arrival_wins() {
        let request = SearchRequest::new(
            "nostr",
            SearchScope::Users,
            SearchTargets::UserPreferred,
            None,
        )
        .expect("query");
        let mut projection = SearchResultsProjection::new(request);
        let hit = event("e1", KIND_PROFILE_METADATA, "nostr user", 100);

        projection.ingest_cache_event(&hit);
        projection.ingest_relay_event(&hit, "wss://relay.example/".to_string());

        let snap = projection.snapshot();
        assert_eq!(snap.hits.len(), 1);
        assert_eq!(snap.hits[0].source, SearchHitSource::Cache);
    }

    #[test]
    fn projection_is_bounded() {
        let request = SearchRequest::new(
            "nostr",
            SearchScope::Kinds(BTreeSet::from([KIND_SHORT_TEXT_NOTE])),
            SearchTargets::UserPreferred,
            Some(2),
        )
        .expect("query");
        let mut projection = SearchResultsProjection::new(request);

        projection.ingest_cache_event(&event("e1", KIND_SHORT_TEXT_NOTE, "nostr one", 1));
        projection.ingest_cache_event(&event("e2", KIND_SHORT_TEXT_NOTE, "nostr two", 2));
        projection.ingest_cache_event(&event("e3", KIND_SHORT_TEXT_NOTE, "nostr three", 3));

        assert_eq!(projection.snapshot().hits.len(), 2);
    }

    #[test]
    fn local_cache_projection_requires_text_match() {
        let request = SearchRequest::new(
            "nostr",
            SearchScope::LongForm,
            SearchTargets::AppDefault,
            None,
        )
        .expect("query");
        let mut projection = SearchResultsProjection::new(request);

        projection.ingest_cache_event(&event("e1", KIND_LONG_FORM, "other text", 100));
        projection.ingest_cache_event(&event("e2", KIND_LONG_FORM, "about nostr", 101));

        assert_eq!(projection.snapshot().hits.len(), 1);
        assert_eq!(projection.snapshot().hits[0].id, "e2");
    }
}
