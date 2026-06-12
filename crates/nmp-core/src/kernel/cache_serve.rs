//! ADR-0045 E1 — Store-cache serve seam.
//!
//! This is the **first half** of the one event-acquisition mechanism:
//! at interest-open / newly-installed time, map the `InterestShape` →
//! one or more `StoreQuery` variants, query the store newest-first up to
//! `replay_limit`, and feed results through the **same post-store
//! projection-dispatch path** relay-delivered events take
//! (`insert_timeline_id_sorted` + `events` read-cache +
//! `notify_event_observers`), skipping `store.insert` entirely.
//!
//! ## Why not `store.insert`
//!
//! A stored event re-entering `insert` returns `InsertOutcome::Duplicate`
//! (it is already on disk), and the `Duplicate` arm deliberately does NOT
//! append to the timeline or fan to observers — it only bumps `relay_count`.
//! Cache-serve must feed the projection-update seam directly (ADR §1.2).
//!
//! ## Budget discipline (the #1085 anti-precedent)
//!
//! `gc_step` (V-117 / #1085) did unbudgeted O(store) scans on the actor
//! thread, blocking the reducer. Cache-serve avoids that mistake:
//!
//! - Each call processes at most `CACHE_SERVE_BUDGET_EVENTS` events total
//!   across all `StoreQuery`s derived from the shape.
//! - The scan is newest-first; `query_visit` early-stops at
//!   `CACHE_SERVE_REPLAY_LIMIT` (≤ `TIMELINE_CACHE_LIMIT = 500`) before
//!   the budget cap applies.
//! - Events are fed oldest-first into `insert_timeline_id_sorted` (the
//!   buffer is collected from newest-first then reversed) so each insert
//!   lands near the tail of the sorted deque and is cheaper on average.
//!
//! ## Dedup safety
//!
//! When relays later deliver the same events, `store.insert` returns
//! `InsertOutcome::Duplicate` and the live path's `Duplicate` arm is a
//! no-op — the event is already in the timeline/cache from serve. Verified
//! in `cache_serve_tests.rs`.
//!
//! ## Completion marker
//!
//! Each completion key (derived from interest scope-key + shape-content
//! hash) is recorded in `served_interest_shapes` after serving. Re-compiles
//! (relay reconnect, follow-list change) do NOT re-serve the same shape —
//! the serve is one-shot per key. Cleared only on account-switch / kernel
//! reset via [`Kernel::clear_served_interest_shapes`].
//!
//! ## Watermark ⇄ serve invariant (ADR §6)
//!
//! > No watermark floor without cache-serve coverage for the same shape.
//!
//! For E1 the covered shapes are exactly the ones the watermark rewrite
//! covers (`AuthorKind` with ≥1 author + ≥1 kind, and `KindTime` with
//! ≥1 kind + 0 authors). A structural assertion in
//! `cache_serve_tests::watermark_invariant_holds` pins this identity.

use super::Kernel;
use crate::planner::InterestShape;
use crate::store::StoreQuery;
use crate::substrate::KernelEvent;

/// Per-call budget: maximum events fed into projections during a single
/// `cache_serve_for_interest` call (summed across all `StoreQuery`s).
/// Conservative: avoids actor stall on a cold kernel with many newly-opened
/// interests in the same tick.
pub(super) const CACHE_SERVE_BUDGET_EVENTS: usize = 200;

/// Maximum events served per interest — bounded by the timeline read-cache
/// limit of 500. Default: fill the visible window in full on the first serve.
pub(super) const CACHE_SERVE_REPLAY_LIMIT: usize = 500;

impl Kernel {
    /// ADR-0045 E1 — serve stored events into the projection-dispatch path
    /// for a newly-opened interest.
    ///
    /// Called from [`Kernel::open_interest_sub`] when `newly_installed` is
    /// true. Idempotent per `completion_key`: calling twice for the same key
    /// is a no-op on the second call (one-shot per (scope, shape-hash)).
    ///
    /// Returns the number of events fed into projections.
    pub(in crate::kernel) fn cache_serve_for_interest(
        &mut self,
        shape: &InterestShape,
        completion_key: u64,
    ) -> usize {
        // One-shot per completion key.
        if self.served_interest_shapes.contains(&completion_key) {
            return 0;
        }

        // Build the StoreQuery list for this shape (E1: AuthorKind + KindTime).
        let queries = shape_to_store_queries(shape);
        if queries.is_empty() {
            // Shape not covered by E1 — mark served anyway so we don't retry.
            self.served_interest_shapes.insert(completion_key);
            return 0;
        }

        let per_query_limit = CACHE_SERVE_REPLAY_LIMIT.min(CACHE_SERVE_BUDGET_EVENTS);
        let mut total_budget_remaining = CACHE_SERVE_BUDGET_EVENTS;

        // Collect events newest-first; we reverse below to feed oldest-first.
        let mut collected: Vec<(
            String,           // id
            String,           // pubkey
            u32,              // kind
            u64,              // created_at
            Vec<Vec<String>>, // tags
            String,           // content
        )> = Vec::new();

        for query in &queries {
            if total_budget_remaining == 0 {
                break;
            }
            let limit = per_query_limit.min(total_budget_remaining);
            // Clone the Arc to allow the immutable borrow of self.store
            // while we also read self.events below in the visitor.
            let store = std::sync::Arc::clone(&self.store);
            let events_cache = &self.events;
            let _ = store.query_visit(query, limit, &mut |ev| {
                if total_budget_remaining == 0 {
                    return std::ops::ControlFlow::Break(());
                }
                // Skip events already in the read-cache — they are already
                // reflected in projections (dedup-safe: the relay Duplicate
                // path handles them when they arrive live).
                if !events_cache.contains_key(&ev.raw.id) {
                    collected.push((
                        ev.raw.id.clone(),
                        ev.raw.pubkey.clone(),
                        ev.raw.kind,
                        ev.raw.created_at,
                        ev.raw.tags.clone(),
                        ev.raw.content.clone(),
                    ));
                    total_budget_remaining -= 1;
                }
                std::ops::ControlFlow::Continue(())
            });
        }

        if collected.is_empty() {
            self.served_interest_shapes.insert(completion_key);
            return 0;
        }

        // Reverse: store returned newest-first; feed oldest-first so each
        // insert lands near the tail of `insert_timeline_id_sorted` (cheaper).
        collected.reverse();

        let count = collected.len();
        for (id, author, kind, created_at, tags, content) in collected {
            let cached = super::types::StoredEvent {
                id: id.clone(),
                author: author.clone(),
                kind,
                created_at,
                tags: tags.clone(),
                content: content.clone(),
                relay_count: 0, // local-store provenance: no relay source yet
            };

            // Incremental diagnostic counters — mirrors ingest_timeline_event.
            self.metric_stored_events = self.metric_stored_events.saturating_add(1);
            if kind == 1 {
                self.metric_note_events = self.metric_note_events.saturating_add(1);
            }
            self.events.insert(id.clone(), cached);
            self.cached_estimated_store_bytes.set(None);

            // Fan to KernelEventObservers — same seam relay-delivered events
            // use after `Inserted | Replaced` (ADR-0045 §2, step 3).
            let kernel_event = KernelEvent {
                id: id.clone(),
                author: author.clone(),
                kind,
                created_at,
                tags,
                content,
            };
            self.notify_event_observers(&kernel_event);

            // Append to timeline if the author is in the follow-set.
            // Mirrors the post-insert branch of `ingest_timeline_event`.
            if self.timeline_authors.contains(&author) {
                self.insert_timeline_id_sorted(id);
            }
        }

        self.changed_since_emit = true;
        self.events_since_last_update =
            self.events_since_last_update.saturating_add(count as u64);

        // Record completion — subsequent calls for this key are no-ops.
        self.served_interest_shapes.insert(completion_key);

        count
    }

    /// Clear the served-interest-shape completion set.
    ///
    /// Must be called on account-switch / kernel reset so the next identity's
    /// interests get a fresh serve.
    pub(in crate::kernel) fn clear_served_interest_shapes(&mut self) {
        self.served_interest_shapes.clear();
    }
}

// ─── shape → StoreQuery mapping (ADR §3, E1 shapes) ─────────────────────────

/// Map an `InterestShape` to the `StoreQuery` variants E1 covers.
///
/// Returns an empty vec when the shape has no E1 mapping. The shapes NOT
/// covered here are:
/// - Wildcard kinds (too broad — no safe bounded index).
/// - Tag-filtered (`#e`, `#p`, addresses, event_ids) — E2/E3.
///
/// E1 coverage:
/// - ≥1 author + ≥1 kind → one `AuthorKind` query per author.
/// - 0 authors + ≥1 kind → `KindTime`.
pub(super) fn shape_to_store_queries(shape: &InterestShape) -> Vec<StoreQuery> {
    // Wildcard kinds: not covered by E1.
    if shape.kinds.is_empty() {
        return Vec::new();
    }

    // Tag-filtered, address-filtered, or event-id-filtered: not covered (E2/E3).
    if !shape.tags.is_empty() || !shape.addresses.is_empty() || !shape.event_ids.is_empty() {
        return Vec::new();
    }

    let kinds: Vec<u32> = shape.kinds.iter().copied().collect();

    if shape.authors.is_empty() {
        // KindTime — global / hashtag feed (0 authors + ≥1 kind).
        vec![StoreQuery::KindTime {
            kinds,
            since: shape.since,
            until: shape.until,
        }]
    } else {
        // AuthorKind — one query per author; results merged under the shared
        // budget. Mirrors the per-author watermark scan `#1091` uses.
        use crate::kernel::hex_to_pubkey_bytes;
        shape
            .authors
            .iter()
            .filter_map(|author_hex| {
                let author = hex_to_pubkey_bytes(author_hex)?;
                Some(StoreQuery::AuthorKind {
                    author,
                    kinds: kinds.clone(),
                    since: shape.since,
                    until: shape.until,
                })
            })
            .collect()
    }
}

/// Derive the completion key for an interest.
///
/// A stable hash of the interest's `SubKey` + the shape's content fields
/// (authors, kinds). `since/until/limit` and routing metadata are excluded —
/// a shape that widens its time window should not retrigger a full re-serve
/// (the watermark+relay refinement handles the delta).
pub(super) fn completion_key_for_interest(
    sub_key: &crate::subs::SubKey,
    shape: &InterestShape,
) -> u64 {
    use crate::stable_hash::stable_hash64;
    let authors: Vec<&str> = shape.authors.iter().map(|s| s.as_str()).collect();
    let kinds: Vec<u32> = shape.kinds.iter().copied().collect();
    stable_hash64((sub_key, &authors, &kinds))
}
