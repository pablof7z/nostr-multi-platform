//! Cache-side full-text serve for search-bearing interest shapes (issue #1811).
//!
//! The structural cache-serve path (`queries.rs` → `StoreQuery` → `serve_chunk`)
//! covers author/kind/tag/address shapes. A *search-bearing* shape
//! (`shape.search.is_some()`) has no `StoreQuery` variant — its matching is the
//! tokenized inverted index, reached through
//! [`nmp_store::EventStore::text_search_visit`]. This module is the bridge.
//!
//! ## Scope resolution — noun-free
//!
//! cache-serve never names a protocol (`nip50`, `nip29`, …). It asks the store
//! which **opaque** scopes have a live cache index and the kinds each indexes
//! ([`nmp_store::EventStore::cache_search_scopes`]). A scope *covers* the shape
//! when its indexed kinds intersect the shape's kinds. The store — not the
//! kernel — owns the scope→kind mapping (the crate-registered
//! `SearchScopeProvider`s were compiled and installed at composition time); the
//! kernel only learns `(SearchScopeId, kinds)` integers (D0).
//!
//! ## Relay-only fallback (point 2)
//!
//! When NO installed scope covers the shape's kinds, this returns `false` and
//! the caller keeps the prior behaviour: no `StoreQuery`, the shape is marked
//! served, and relays deliver via NIP-50. So a search shape is cache-covered
//! **iff** a cache scope is registered for its kinds; otherwise it is relay-only.
//!
//! ## No network work (point 3)
//!
//! This path only reads the local store (`text_search_visit` + `peek_by_id`).
//! It opens no relay subscription — a `CacheOnly`-mode scope therefore performs
//! zero network work here, and the relay fan-out (when a scope's `cache_mode`
//! allows it) is a wholly separate planner/routing concern. cache-serve is
//! store-only by construction.

use super::super::Kernel;
use super::continuation::CollectedEvent;
use crate::planner::InterestShape;
use crate::store::{
    SearchScopeId, TextSearchBudget, TextSearchHit, TextSearchOrder, TextSearchQuery,
    TextSearchStatus,
};
use std::collections::BTreeSet;
use std::ops::ControlFlow;
use std::sync::Arc;

impl Kernel {
    /// Resolve the installed cache scopes whose indexed kinds intersect the
    /// shape's kinds, returning `(scope_id, kinds_to_query)` for each — where
    /// `kinds_to_query` is the intersection (so the index is asked only for the
    /// kinds it actually holds for this interest). Empty when no scope covers
    /// the shape ⇒ the caller falls back to relay-only serve.
    ///
    /// An empty `shape.kinds` (kinds-wildcard) is intentionally NOT matched: a
    /// wildcard search would fan across every scope's whole corpus, which the
    /// structural path already refuses as an unbounded scan. A search shape that
    /// reaches here without kinds stays relay-only.
    fn resolve_cache_search_scopes(
        &self,
        shape: &InterestShape,
    ) -> Vec<(SearchScopeId, BTreeSet<u32>)> {
        if shape.kinds.is_empty() {
            return Vec::new();
        }
        self.store
            .cache_search_scopes()
            .into_iter()
            .filter_map(|(scope, scope_kinds)| {
                let overlap: BTreeSet<u32> = scope_kinds
                    .intersection(&shape.kinds)
                    .copied()
                    .collect();
                if overlap.is_empty() {
                    None
                } else {
                    Some((scope, overlap))
                }
            })
            .collect()
    }

    /// Attempt to serve a search-bearing shape from the local cache index.
    ///
    /// Returns `true` when at least one registered cache scope covered the
    /// shape's kinds — the shape is cache-served (hits fed into the same
    /// post-store projection path structural serves use) and the caller records
    /// the completion key. Returns `false` when no scope matched, so the caller
    /// keeps the relay-only behaviour (mark served, relays deliver via NIP-50).
    ///
    /// Bounded by construction: `text_search_visit` enforces a
    /// [`TextSearchBudget`] (no hidden full-corpus scan) and we stop feeding at
    /// the consumer's visible window via [`Kernel::serve_depth_for_shape`].
    pub(super) fn try_cache_serve_search(
        &mut self,
        shape: &InterestShape,
        completion_key: u64,
    ) -> bool {
        let Some(query_text) = shape.search.clone() else {
            return false;
        };
        let scopes = self.resolve_cache_search_scopes(shape);
        if scopes.is_empty() {
            // No cache scope registered for these kinds → relay-only.
            return false;
        }

        let depth = self.serve_depth_for_shape(shape);
        let mut served = 0usize;

        for (scope, kinds) in scopes {
            if served >= depth {
                break;
            }
            let remaining = depth - served;
            let query = TextSearchQuery {
                scope,
                query: query_text.clone(),
                kinds,
                since: shape.since,
                until: shape.until,
                limit: remaining,
                order: TextSearchOrder::NewestFirst,
                budget: TextSearchBudget::default(),
            };

            // Phase 1 — collect matching event ids (immutable borrow of store).
            // The visitor caps itself at `remaining`; the store's own budget is
            // the hard ceiling. We only collect ids here, then hydrate + feed in
            // a second phase to avoid holding the store lock across `peek_by_id`.
            let mut hit_ids: Vec<crate::store::EventId> = Vec::new();
            {
                let store = Arc::clone(&self.store);
                let _status: TextSearchStatus = store
                    .text_search_visit(&query, &mut |hit: TextSearchHit| {
                        if let Some(event_id) = hit.event_id {
                            hit_ids.push(event_id);
                        }
                        if hit_ids.len() >= remaining {
                            ControlFlow::Break(())
                        } else {
                            ControlFlow::Continue(())
                        }
                    })
                    .unwrap_or(TextSearchStatus::StoreError);
            }

            // Phase 2 — hydrate each hit and feed it through the shared
            // post-store projection path (`feed_served_event`), oldest-first so
            // each insert lands near the timeline tail.
            let mut collected: Vec<CollectedEvent> = Vec::new();
            for event_id in hit_ids {
                let Ok(Some(stored)) = self.store.peek_by_id(&event_id) else {
                    continue;
                };
                let raw = &stored.raw;
                // Live→serve dedup: already reflected in projections.
                if self.events.contains_key(&raw.id) {
                    continue;
                }
                collected.push(CollectedEvent {
                    id: raw.id.clone(),
                    author: raw.pubkey.clone(),
                    kind: raw.kind,
                    created_at: raw.created_at,
                    tags: raw.tags.clone(),
                    content: raw.content.clone(),
                    sig: raw.sig.clone(),
                });
            }
            collected.reverse();
            for ev in collected {
                self.feed_served_event(ev);
                served += 1;
            }
        }

        if served > 0 {
            self.changed_since_emit = true;
            self.events_since_last_update = self
                .events_since_last_update
                .saturating_add(served as u64);
        }

        // Covered by the cache (even if zero hits matched the query text): a
        // registered scope owns these kinds, so this is NOT a relay-only shape.
        self.served_interest_shapes.insert(completion_key);
        true
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
